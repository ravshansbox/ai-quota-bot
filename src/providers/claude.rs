use crate::{
    error::AppResult,
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot, WindowKind},
    providers::{ProviderRequestError, QuotaProvider},
};
use anyhow::{Context, anyhow};
use async_trait::async_trait;
use reqwest::{Client, StatusCode};
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

#[derive(Debug, Deserialize)]
struct UsageResponse {
    plan: String,
    window_kind: String,
    window_id: Option<String>,
    reset_at: String,
    usage: Option<u64>,
    limit: Option<u64>,
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
            .get(format!("{}/usage", self.base_url))
            .bearer_auth(&creds.access_token)
            .send()
            .await
            .map_err(|err| ProviderRequestError::Other(err.into()))?;

        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ProviderRequestError::Authentication);
        }

        let response = response
            .error_for_status()
            .map_err(|err| ProviderRequestError::Other(err.into()))?;
        let payload: UsageResponse = response
            .json()
            .await
            .map_err(|err| ProviderRequestError::Other(err.into()))?;
        let window_kind =
            parse_window_kind(&payload.window_kind).map_err(ProviderRequestError::Other)?;
        let reset_at = OffsetDateTime::parse(&payload.reset_at, &Rfc3339)
            .context("failed to parse Claude reset_at as RFC3339")
            .map_err(ProviderRequestError::Other)?;

        Ok(vec![QuotaSnapshot {
            provider: ProviderKind::Claude,
            plan: payload.plan,
            window_kind,
            window_id: payload.window_id,
            reset_at,
            usage: payload.usage,
            limit: payload.limit,
        }])
    }

    async fn refresh_credentials(
        &self,
        creds: &ProviderCredentials,
    ) -> AppResult<ProviderCredentials> {
        Ok(creds.clone())
    }
}

fn parse_window_kind(raw: &str) -> anyhow::Result<WindowKind> {
    match raw {
        "5h" => Ok(WindowKind::FiveHours),
        "7d" => Ok(WindowKind::SevenDays),
        other => Err(anyhow!("unsupported Claude window kind: {other}"))
            .context("failed to parse Claude usage response"),
    }
}
