use crate::{
    error::AppResult,
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot, WindowKind},
    providers::{ProviderRequestError, QuotaProvider},
};
use anyhow::{Context, anyhow};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Clone, Debug)]
pub struct ClaudeProvider {
    client: Client,
    base_url: String,
}

impl ClaudeProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self::with_client(Client::new(), base_url)
    }

    pub fn with_client(client: Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
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
            .header("anthropic-beta", "oauth-2025-04-20")
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
                plan: "max".into(),
                window_kind: WindowKind::FiveHours,
                window_id: Some("5h".into()),
                reset_at,
                usage: Some(entry.utilization as u64),
                limit: Some(100),
            });
        }

        if let Some(entry) = payload.seven_day {
            let reset_at = parse_reset_at(entry.resets_at.as_deref())?;
            snapshots.push(QuotaSnapshot {
                provider: ProviderKind::Claude,
                plan: "max".into(),
                window_kind: WindowKind::SevenDays,
                window_id: Some("7d".into()),
                reset_at,
                usage: Some(entry.utilization as u64),
                limit: Some(100),
            });
        }

        Ok(snapshots)
    }

    async fn refresh_credentials(
        &self,
        creds: &ProviderCredentials,
    ) -> AppResult<ProviderCredentials> {
        Ok(creds.clone())
    }
}

fn parse_reset_at(raw: Option<&str>) -> Result<OffsetDateTime, ProviderRequestError> {
    match raw {
        Some(s) => OffsetDateTime::parse(s, &Rfc3339)
            .context("invalid reset_at RFC3339")
            .map_err(ProviderRequestError::Other),
        None => Ok(OffsetDateTime::now_utc()),
    }
}
