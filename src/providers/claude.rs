use crate::{
    auth,
    error::AppResult,
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot, WindowKind},
    providers::{ProviderRequestError, QuotaProvider},
};
use anyhow::{Context, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Clone, Debug)]
pub struct ClaudeProvider {
    client: Client,
    base_url: String,
    auth_path: Option<PathBuf>,
}

impl ClaudeProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_client(Client::new(), base_url)
    }

    pub fn with_client(client: Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            auth_path: None,
        }
    }

    pub fn with_auth_path(self, path: PathBuf) -> Self {
        Self {
            auth_path: Some(path),
            ..self
        }
    }
}

/// Real Anthropic OAuth usage endpoint
/// https://api.anthropic.com/api/oauth/usage
#[derive(Debug, Deserialize)]
struct AnthropicUsageResponse {
    five_hour: Option<WindowEntry>,
    seven_day: Option<WindowEntry>,
}

#[derive(Debug, Deserialize)]
struct WindowEntry {
    utilization: f64,
    #[serde(rename = "resets_at")]
    resets_at: Option<String>,
}

#[async_trait]
impl QuotaProvider for ClaudeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    async fn fetch_snapshots(
        &self,
        creds: &ProviderCredentials,
    ) -> Result<Vec<QuotaSnapshot>, ProviderRequestError> {
        let response = self
            .client
            .get(format!("{}/api/oauth/usage", self.base_url))
            .bearer_auth(&creds.access_token)
            .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20")
            .send()
            .await
            .map_err(|err| ProviderRequestError::Other(err.into()))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderRequestError::Authentication);
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ProviderRequestError::Other(anyhow!(
                "usage endpoint not found at {}/api/oauth/usage",
                self.base_url
            )));
        }

        let payload: AnthropicUsageResponse = response
            .error_for_status()
            .map_err(|err| ProviderRequestError::Other(err.into()))?
            .json()
            .await
            .map_err(|err| ProviderRequestError::Other(err.into()))?;

        let mut snapshots = Vec::new();

        if let Some(entry) = payload.five_hour {
            let reset_at = parse_reset_at(entry.resets_at.as_deref())?;
            snapshots.push(QuotaSnapshot {
                provider: ProviderKind::Claude,
                window_kind: WindowKind::FiveHours,
                window_id: Some("5h".into()),
                reset_at,
                usage: Some(clamp_percentage(entry.utilization)),
                limit: Some(100),
                resets_available: 0,
            });
        }

        if let Some(entry) = payload.seven_day {
            let reset_at = parse_reset_at(entry.resets_at.as_deref())?;
            snapshots.push(QuotaSnapshot {
                provider: ProviderKind::Claude,
                window_kind: WindowKind::SevenDays,
                window_id: Some("7d".into()),
                reset_at,
                usage: Some(clamp_percentage(entry.utilization)),
                limit: Some(100),
                resets_available: 0,
            });
        }

        Ok(snapshots)
    }

    async fn refresh_credentials(
        &self,
        creds: &ProviderCredentials,
    ) -> AppResult<ProviderCredentials> {
        let refresh_token = creds
            .refresh_token
            .as_deref()
            .ok_or_else(|| anyhow!("no refresh token for Claude"))?;

        let resp: serde_json::Value = self
            .client
            .post(format!("{}/v1/oauth/token", self.base_url))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", "9d1c250a-e61b-44d9-88ed-5944d1962f5e"),
            ])
            .send()
            .await
            .context("Claude token refresh request failed")?
            .error_for_status()
            .context("Claude token refresh rejected")?
            .json()
            .await
            .context("Claude token refresh response parse failed")?;

        let new_access = resp
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("missing access_token in Claude refresh response"))?
            .to_string();

        let new_refresh = resp
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let expires_at = resp
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .map(|secs| OffsetDateTime::now_utc() + time::Duration::seconds(secs));

        let updated = ProviderCredentials {
            access_token: new_access,
            refresh_token: new_refresh.or_else(|| creds.refresh_token.clone()),
            expires_at,
            account_id: creds.account_id.clone(),
        };

        // Persist back to auth.json so the new tokens survive restart
        if let Some(ref auth_path) = self.auth_path
            && let Err(e) = auth::persist_credentials(auth_path, ProviderKind::Claude, &updated)
        {
            tracing::warn!(error = %e, "failed to persist refreshed Claude credentials");
        }

        tracing::info!("Claude token refreshed successfully");
        Ok(updated)
    }
}

/// Clamp a raw percentage value into the valid `0..=100` range.
fn clamp_percentage(value: f64) -> u64 {
    value.clamp(0.0, 100.0) as u64
}

/// Parse `resets_at` as RFC 3339. Returns `Ok(None)` when the field is absent
/// so callers can report the window with an "unknown" reset time instead of
/// faking a moving timestamp.
fn parse_reset_at(raw: Option<&str>) -> Result<Option<OffsetDateTime>, ProviderRequestError> {
    match raw {
        Some(s) => OffsetDateTime::parse(s, &Rfc3339)
            .map(Some)
            .context("invalid reset_at RFC3339")
            .map_err(ProviderRequestError::Other),
        None => Ok(None),
    }
}
