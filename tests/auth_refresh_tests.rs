use ai_quota_bot::{
    auth_refresh::{fetch_with_refresh, should_refresh},
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot, WindowKind},
    providers::{ProviderRequestError, QuotaProvider},
};
use anyhow::anyhow;
use async_trait::async_trait;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use time::macros::datetime;
use time::{Duration, OffsetDateTime};

fn credentials(expires_at: Option<OffsetDateTime>) -> ProviderCredentials {
    ProviderCredentials {
        access_token: "token".into(),
        refresh_token: Some("refresh".into()),
        expires_at,
        account_id: None,
    }
}

fn snapshot() -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Claude,
        plan: "max".into(),
        window_kind: WindowKind::FiveHours,
        window_id: Some("window-a".into()),
        reset_at: datetime!(2026-06-29 17:00 UTC),
        usage: Some(10),
        limit: Some(100),
    }
}

#[derive(Clone)]
struct MockProvider {
    state: Arc<Mutex<MockState>>,
}

struct MockState {
    fetch_results: VecDeque<Result<Vec<QuotaSnapshot>, ProviderRequestError>>,
    refreshed_credentials: Option<ProviderCredentials>,
    refresh_error: Option<String>,
    fetch_tokens: Vec<String>,
    refresh_tokens: Vec<String>,
}

impl MockProvider {
    fn new(fetch_results: Vec<Result<Vec<QuotaSnapshot>, ProviderRequestError>>) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                fetch_results: fetch_results.into(),
                refreshed_credentials: None,
                refresh_error: None,
                fetch_tokens: Vec::new(),
                refresh_tokens: Vec::new(),
            })),
        }
    }

    fn with_refreshed_credentials(self, refreshed_credentials: ProviderCredentials) -> Self {
        self.state.lock().unwrap().refreshed_credentials = Some(refreshed_credentials);
        self
    }

    fn with_refresh_error(self, message: &str) -> Self {
        self.state.lock().unwrap().refresh_error = Some(message.into());
        self
    }

    fn fetch_tokens(&self) -> Vec<String> {
        self.state.lock().unwrap().fetch_tokens.clone()
    }

    fn refresh_tokens(&self) -> Vec<String> {
        self.state.lock().unwrap().refresh_tokens.clone()
    }
}

#[async_trait]
impl QuotaProvider for MockProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    async fn fetch_snapshots(
        &self,
        creds: &ProviderCredentials,
    ) -> Result<Vec<QuotaSnapshot>, ProviderRequestError> {
        let mut state = self.state.lock().unwrap();
        state.fetch_tokens.push(creds.access_token.clone());
        state
            .fetch_results
            .pop_front()
            .unwrap_or_else(|| Ok(vec![snapshot()]))
    }

    async fn refresh_credentials(
        &self,
        creds: &ProviderCredentials,
    ) -> anyhow::Result<ProviderCredentials> {
        let mut state = self.state.lock().unwrap();
        state.refresh_tokens.push(creds.access_token.clone());

        if let Some(message) = &state.refresh_error {
            return Err(anyhow!(message.clone()));
        }

        Ok(state
            .refreshed_credentials
            .clone()
            .unwrap_or_else(|| ProviderCredentials {
                access_token: format!("{}-refreshed", creds.access_token),
                refresh_token: creds.refresh_token.clone(),
                expires_at: creds.expires_at,
                account_id: creds.account_id.clone(),
            }))
    }
}

#[test]
fn refresh_is_required_when_expiry_is_within_five_minutes() {
    let creds = credentials(Some(datetime!(2026-06-29 12:05 UTC)));

    assert!(should_refresh(
        &creds,
        datetime!(2026-06-29 12:01 UTC),
        Duration::minutes(5)
    ));
}

#[test]
fn refresh_is_not_required_without_expiry() {
    let creds = credentials(None);

    assert!(!should_refresh(
        &creds,
        datetime!(2026-06-29 12:01 UTC),
        Duration::minutes(5)
    ));
}

#[test]
fn refresh_is_not_required_outside_leeway() {
    let creds = credentials(Some(datetime!(2026-06-29 12:06 UTC)));

    assert!(!should_refresh(
        &creds,
        datetime!(2026-06-29 12:00 UTC),
        Duration::minutes(5)
    ));
}

#[tokio::test]
async fn fetch_with_refresh_uses_existing_credentials_when_token_is_fresh() {
    let provider = MockProvider::new(vec![Ok(vec![snapshot()])]);
    let creds = credentials(Some(datetime!(2026-06-29 12:30 UTC)));

    let (returned_creds, snapshots) =
        fetch_with_refresh(&provider, &creds, datetime!(2026-06-29 12:00 UTC))
            .await
            .unwrap();

    assert_eq!(returned_creds, creds);
    assert_eq!(snapshots, vec![snapshot()]);
    assert_eq!(provider.refresh_tokens(), Vec::<String>::new());
    assert_eq!(provider.fetch_tokens(), vec!["token".to_string()]);
}

#[tokio::test]
async fn fetch_with_refresh_refreshes_before_fetch_when_expiry_is_near() {
    let refreshed = ProviderCredentials {
        access_token: "token-refreshed".into(),
        refresh_token: Some("refresh".into()),
        expires_at: Some(datetime!(2026-06-29 13:00 UTC)),
        account_id: None,
    };
    let provider =
        MockProvider::new(vec![Ok(vec![snapshot()])]).with_refreshed_credentials(refreshed.clone());
    let creds = credentials(Some(datetime!(2026-06-29 12:04 UTC)));

    let (returned_creds, snapshots) =
        fetch_with_refresh(&provider, &creds, datetime!(2026-06-29 12:00 UTC))
            .await
            .unwrap();

    assert_eq!(returned_creds, refreshed);
    assert_eq!(snapshots, vec![snapshot()]);
    assert_eq!(provider.refresh_tokens(), vec!["token".to_string()]);
    assert_eq!(provider.fetch_tokens(), vec!["token-refreshed".to_string()]);
}

#[tokio::test]
async fn fetch_with_refresh_retries_after_authentication_failure() {
    let refreshed = ProviderCredentials {
        access_token: "token-refreshed".into(),
        refresh_token: Some("refresh".into()),
        expires_at: Some(datetime!(2026-06-29 13:00 UTC)),
        account_id: None,
    };
    let provider = MockProvider::new(vec![
        Err(ProviderRequestError::Authentication),
        Ok(vec![snapshot()]),
    ])
    .with_refreshed_credentials(refreshed.clone());
    let creds = credentials(Some(datetime!(2026-06-29 12:30 UTC)));

    let (returned_creds, snapshots) =
        fetch_with_refresh(&provider, &creds, datetime!(2026-06-29 12:00 UTC))
            .await
            .unwrap();

    assert_eq!(returned_creds, refreshed);
    assert_eq!(snapshots, vec![snapshot()]);
    assert_eq!(provider.refresh_tokens(), vec!["token".to_string()]);
    assert_eq!(
        provider.fetch_tokens(),
        vec!["token".to_string(), "token-refreshed".to_string()]
    );
}

#[tokio::test]
async fn fetch_with_refresh_returns_refresh_error() {
    let provider = MockProvider::new(vec![]).with_refresh_error("refresh failed");
    let creds = credentials(Some(datetime!(2026-06-29 12:04 UTC)));

    let error = fetch_with_refresh(&provider, &creds, datetime!(2026-06-29 12:00 UTC))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("refresh failed"));
}

#[tokio::test]
async fn fetch_with_refresh_returns_non_auth_fetch_error() {
    let provider = MockProvider::new(vec![Err(ProviderRequestError::Other(anyhow!("boom")))]);
    let creds = credentials(Some(datetime!(2026-06-29 12:30 UTC)));

    let error = fetch_with_refresh(&provider, &creds, datetime!(2026-06-29 12:00 UTC))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("boom"));
    assert_eq!(provider.refresh_tokens(), Vec::<String>::new());
    assert_eq!(provider.fetch_tokens(), vec!["token".to_string()]);
}
