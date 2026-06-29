use crate::{
    error::AppResult,
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot, WindowKind},
    providers::{ProviderRequestError, QuotaProvider},
};
use anyhow::anyhow;
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Clone, Debug)]
pub struct CodexProvider {
    client: Client,
    base_url: String,
}

impl CodexProvider {
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

/// Real Codex usage endpoint
/// https://chatgpt.com/backend-api/wham/usage
#[derive(Debug, Deserialize)]
struct CodexUsageResponse {
    #[serde(rename = "rate_limit", alias = "rate_limits")]
    rate_limit: Option<RateLimit>,
    #[allow(dead_code)]
    credits: Option<Credits>,
    #[serde(rename = "spend_control")]
    #[allow(dead_code)]
    spend_control: Option<SpendControl>,
}

#[derive(Debug, Deserialize)]
struct RateLimit {
    #[serde(rename = "primary_window", alias = "primary")]
    primary_window: Option<RateLimitWindow>,
    #[serde(rename = "secondary_window", alias = "secondary")]
    secondary_window: Option<RateLimitWindow>,
    #[serde(rename = "five_hour_limit", alias = "five_hour")]
    five_hour_limit: Option<RateLimitWindow>,
}

#[derive(Debug, Deserialize)]
struct RateLimitWindow {
    #[serde(rename = "used_percent")]
    used_percent: Option<f64>,
    #[serde(rename = "reset_at")]
    reset_at: Option<serde_json::Value>, // can be i64 (epoch) or string (ISO)
    #[serde(rename = "limit_window_seconds")]
    #[allow(dead_code)]
    limit_window_seconds: Option<u64>,
    #[serde(rename = "reset_time_ms")]
    #[allow(dead_code)]
    reset_time_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Credits {
    #[serde(rename = "has_credits")]
    #[allow(dead_code)]
    has_credits: Option<bool>,
    #[allow(dead_code)]
    balance: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct SpendControl {
    #[allow(dead_code)]
    reached: Option<bool>,
}

fn parse_reset_timestamp(value: Option<serde_json::Value>) -> Option<OffsetDateTime> {
    match value {
        Some(serde_json::Value::Number(n)) => {
            let secs = n.as_i64()?;
            OffsetDateTime::from_unix_timestamp(secs).ok()
        }
        Some(serde_json::Value::String(s)) => OffsetDateTime::parse(&s, &Rfc3339).ok(),
        _ => None,
    }
}

#[async_trait]
impl QuotaProvider for CodexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Codex
    }

    async fn fetch_snapshots(
        &self,
        creds: &ProviderCredentials,
    ) -> Result<Vec<QuotaSnapshot>, ProviderRequestError> {
        let account_id = creds
            .account_id
            .as_deref()
            .ok_or_else(|| ProviderRequestError::Other(anyhow!("missing Codex account id")))?;

        let response = self
            .client
            .get(format!("{}/backend-api/wham/usage", self.base_url))
            .bearer_auth(&creds.access_token)
            .header("ChatGPT-Account-Id", account_id)
            .header("Origin", "https://chatgpt.com")
            .header("Referer", "https://chatgpt.com/")
            .send()
            .await
            .map_err(|err| ProviderRequestError::Other(err.into()))?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderRequestError::Authentication);
        }

        let payload: CodexUsageResponse = response
            .error_for_status()
            .map_err(|err| ProviderRequestError::Other(err.into()))?
            .json()
            .await
            .map_err(|err| ProviderRequestError::Other(err.into()))?;

        let mut snapshots = Vec::new();

        // Try to extract the 5h primary window
        let five_hour = payload
            .rate_limit
            .as_ref()
            .and_then(|rl| rl.primary_window.as_ref())
            .or_else(|| {
                payload
                    .rate_limit
                    .as_ref()
                    .and_then(|rl| rl.five_hour_limit.as_ref())
            });

        if let Some(window) = five_hour {
            let used = window.used_percent.unwrap_or(0.0) as u64;
            let reset_at = parse_reset_timestamp(window.reset_at.clone().or_else(|| {
                window
                    .reset_time_ms
                    .map(|ms| serde_json::Value::Number(serde_json::Number::from(ms / 1000)))
            }))
            .unwrap_or_else(OffsetDateTime::now_utc);

            snapshots.push(QuotaSnapshot {
                provider: ProviderKind::Codex,
                plan: "pro".into(),
                window_kind: WindowKind::FiveHours,
                window_id: Some(format!("5h-{}", reset_at.unix_timestamp())),
                reset_at,
                usage: Some(used),
                limit: Some(100),
            });
        }

        // Try to extract the 7d secondary window
        if let Some(window) = payload
            .rate_limit
            .as_ref()
            .and_then(|rl| rl.secondary_window.as_ref())
        {
            let used = window.used_percent.unwrap_or(0.0) as u64;
            let reset_at = parse_reset_timestamp(window.reset_at.clone().or_else(|| {
                window
                    .reset_time_ms
                    .map(|ms| serde_json::Value::Number(serde_json::Number::from(ms / 1000)))
            }))
            .unwrap_or_else(OffsetDateTime::now_utc);

            snapshots.push(QuotaSnapshot {
                provider: ProviderKind::Codex,
                plan: "pro".into(),
                window_kind: WindowKind::SevenDays,
                window_id: Some(format!("7d-{}", reset_at.unix_timestamp())),
                reset_at,
                usage: Some(used),
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
