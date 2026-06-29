mod support;

use ai_quota_bot::{
    config::AppConfig,
    daemon::Daemon,
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot, ResetEvent, WindowKind},
    providers::{
        ProviderRequestError, QuotaProvider, claude::ClaudeProvider, codex::CodexProvider,
    },
    telegram::ResetNotifier,
};
use anyhow::Result;
use async_trait::async_trait;
use httpmock::{Method::GET, MockServer};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn credentials() -> ProviderCredentials {
    ProviderCredentials {
        access_token: "token".into(),
        refresh_token: Some("refresh".into()),
        expires_at: None,
        account_id: None,
        raw_source: HashMap::new(),
    }
}

#[derive(Clone, Default)]
struct FakeNotifier {
    sent: Arc<Mutex<Vec<ResetEvent>>>,
}

#[async_trait]
impl ResetNotifier for FakeNotifier {
    async fn notify_reset(&self, event: &ResetEvent) -> Result<()> {
        self.sent.lock().unwrap().push(event.clone());
        Ok(())
    }
}

#[derive(Clone)]
struct FakeProvider {
    provider: ProviderKind,
}

#[async_trait]
impl QuotaProvider for FakeProvider {
    fn kind(&self) -> ProviderKind {
        self.provider
    }

    async fn fetch_snapshots(
        &self,
        _creds: &ProviderCredentials,
    ) -> Result<Vec<QuotaSnapshot>, ProviderRequestError> {
        Ok(Vec::new())
    }

    async fn refresh_credentials(
        &self,
        creds: &ProviderCredentials,
    ) -> Result<ProviderCredentials> {
        Ok(creds.clone())
    }
}

#[test]
fn provider_window_kinds_cover_supported_reset_windows() {
    assert_eq!(ProviderKind::Claude.as_str(), "claude");
    assert_eq!(WindowKind::SevenDays.as_str(), "7d");
}

#[test]
fn detector_can_be_embedded_in_daemon_state() {
    let config = AppConfig {
        telegram_bot_token: "bot-token".into(),
        telegram_chat_id: "1234".into(),
        auth_path: PathBuf::from("/tmp/auth.json"),
        poll_interval_secs: 600,
    };

    let mut daemon = Daemon::new(
        config,
        FakeNotifier::default(),
        FakeProvider {
            provider: ProviderKind::Claude,
        },
        FakeProvider {
            provider: ProviderKind::Codex,
        },
    );

    assert!(daemon.detector.detect(Vec::new()).is_empty());
}

#[tokio::test]
async fn claude_adapter_parses_five_hour_window() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage")
            .header("authorization", "Bearer token");
        then.status(200)
            .header("content-type", "application/json")
            .body(support::claude_usage_response("2026-06-29T17:00:00Z"));
    });

    let provider = ClaudeProvider::new(server.url(""));
    let snapshots = provider.fetch_snapshots(&credentials()).await.unwrap();

    mock.assert();
    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.provider, ProviderKind::Claude);
    assert_eq!(snapshot.plan, "max");
    assert_eq!(snapshot.window_kind, WindowKind::FiveHours);
    assert_eq!(snapshot.window_id.as_deref(), Some("claude-window"));
    assert_eq!(snapshot.usage, Some(12));
    assert_eq!(snapshot.limit, Some(100));
}

#[tokio::test]
async fn codex_adapter_parses_seven_day_window() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/usage")
            .header("authorization", "Bearer token");
        then.status(200)
            .header("content-type", "application/json")
            .body(support::codex_usage_response("2026-06-30T00:00:00Z"));
    });

    let provider = CodexProvider::new(server.url(""));
    let snapshots = provider.fetch_snapshots(&credentials()).await.unwrap();

    mock.assert();
    assert_eq!(snapshots.len(), 1);
    let snapshot = &snapshots[0];
    assert_eq!(snapshot.provider, ProviderKind::Codex);
    assert_eq!(snapshot.plan, "pro");
    assert_eq!(snapshot.window_kind, WindowKind::SevenDays);
    assert_eq!(snapshot.window_id.as_deref(), Some("codex-window"));
    assert_eq!(snapshot.usage, Some(3));
    assert_eq!(snapshot.limit, Some(50));
}

#[tokio::test]
async fn unauthorized_provider_response_maps_to_authentication_error() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET).path("/usage");
        then.status(401);
    });

    let provider = ClaudeProvider::new(server.url(""));
    let error = provider.fetch_snapshots(&credentials()).await.unwrap_err();

    mock.assert();
    assert!(matches!(error, ProviderRequestError::Authentication));
}
