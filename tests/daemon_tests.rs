mod support;

use ai_quota_bot::{
    config::AppConfig,
    daemon::Daemon,
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot, ResetEvent, WindowKind},
    providers::{
        ProviderRequestError, QuotaProvider, claude::ClaudeProvider, codex::CodexProvider,
    },
    telegram::{ResetNotifier, format_reset_message},
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use httpmock::{Method::GET, MockServer};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tempfile::TempDir;
use time::OffsetDateTime;
use time::macros::datetime;

fn credentials() -> ProviderCredentials {
    ProviderCredentials {
        access_token: "token".into(),
        refresh_token: Some("refresh".into()),
        expires_at: None,
        account_id: None,
        raw_source: HashMap::new(),
    }
}

fn claude_snapshot(reset_at: OffsetDateTime, window_id: &str) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Claude,
        plan: "max".into(),
        window_kind: WindowKind::FiveHours,
        window_id: Some(window_id.into()),
        reset_at,
        usage: Some(1),
        limit: Some(10),
    }
}

fn codex_snapshot(reset_at: OffsetDateTime, window_id: &str) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Codex,
        plan: "pro".into(),
        window_kind: WindowKind::SevenDays,
        window_id: Some(window_id.into()),
        reset_at,
        usage: Some(3),
        limit: Some(50),
    }
}

#[derive(Clone, Default)]
struct FakeNotifier {
    sent: Arc<Mutex<Vec<String>>>,
}

impl FakeNotifier {
    fn messages(&self) -> Vec<String> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl ResetNotifier for FakeNotifier {
    async fn notify_reset(&self, event: &ResetEvent) -> Result<()> {
        self.sent.lock().unwrap().push(format_reset_message(event));
        Ok(())
    }
}

#[derive(Clone)]
struct FakeProvider {
    provider: ProviderKind,
    state: Arc<Mutex<FakeProviderState>>,
}

struct FakeProviderState {
    fetch_results: VecDeque<Result<Vec<QuotaSnapshot>, ProviderRequestError>>,
    refreshed_credentials: Option<ProviderCredentials>,
    fetch_tokens: Vec<String>,
    refresh_tokens: Vec<String>,
    refresh_count: usize,
}

impl FakeProvider {
    fn new(
        provider: ProviderKind,
        fetch_results: Vec<Result<Vec<QuotaSnapshot>, ProviderRequestError>>,
    ) -> Self {
        Self {
            provider,
            state: Arc::new(Mutex::new(FakeProviderState {
                fetch_results: fetch_results.into(),
                refreshed_credentials: None,
                fetch_tokens: Vec::new(),
                refresh_tokens: Vec::new(),
                refresh_count: 0,
            })),
        }
    }

    fn with_refreshed_credentials(self, refreshed_credentials: ProviderCredentials) -> Self {
        self.state.lock().unwrap().refreshed_credentials = Some(refreshed_credentials);
        self
    }

    fn fetch_tokens(&self) -> Vec<String> {
        self.state.lock().unwrap().fetch_tokens.clone()
    }

    fn refresh_tokens(&self) -> Vec<String> {
        self.state.lock().unwrap().refresh_tokens.clone()
    }

    fn refresh_count(&self) -> usize {
        self.state.lock().unwrap().refresh_count
    }
}

#[async_trait]
impl QuotaProvider for FakeProvider {
    fn kind(&self) -> ProviderKind {
        self.provider
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
            .unwrap_or_else(|| Ok(Vec::new()))
    }

    async fn refresh_credentials(
        &self,
        creds: &ProviderCredentials,
    ) -> Result<ProviderCredentials> {
        let mut state = self.state.lock().unwrap();
        state.refresh_tokens.push(creds.access_token.clone());
        state.refresh_count += 1;
        Ok(state
            .refreshed_credentials
            .clone()
            .unwrap_or_else(|| ProviderCredentials {
                access_token: format!("{}-refreshed", creds.access_token),
                refresh_token: creds.refresh_token.clone(),
                expires_at: creds.expires_at,
                account_id: creds.account_id.clone(),
                raw_source: creds.raw_source.clone(),
            }))
    }
}

fn app_config(auth_path: PathBuf) -> AppConfig {
    AppConfig {
        telegram_bot_token: "bot-token".into(),
        telegram_chat_id: "1234".into(),
        auth_path,
        poll_interval_secs: 600,
    }
}

fn write_auth_file(path: &Path, anthropic_access_token: &str, codex_access_token: &str) {
    let body = format!(
        r#"{{
  "anthropic": {{
    "type": "oauth",
    "access": "{anthropic_access_token}",
    "refresh": "claude-refresh",
    "expires": null
  }},
  "openai-codex": {{
    "type": "oauth",
    "access": "{codex_access_token}",
    "refresh": "codex-refresh",
    "expires": null,
    "accountId": "codex-account"
  }}
}}"#
    );

    fs::write(path, body).unwrap();
}

fn temp_auth_file() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("auth.json");
    (dir, path)
}

#[test]
fn provider_window_kinds_cover_supported_reset_windows() {
    assert_eq!(ProviderKind::Claude.as_str(), "claude");
    assert_eq!(WindowKind::SevenDays.as_str(), "7d");
}

#[test]
fn detector_can_be_embedded_in_daemon_state() {
    let mut daemon = Daemon::new(
        app_config(PathBuf::from("/tmp/auth.json")),
        FakeNotifier::default(),
        FakeProvider::new(ProviderKind::Claude, vec![]),
        FakeProvider::new(ProviderKind::Codex, vec![]),
    );

    assert!(daemon.detector.detect(Vec::new()).is_empty());
}

#[tokio::test]
async fn first_successful_cycle_sends_no_notifications() {
    let (_dir, auth_path) = temp_auth_file();
    write_auth_file(&auth_path, "claude-token", "codex-token");

    let notifier = FakeNotifier::default();
    let claude = FakeProvider::new(
        ProviderKind::Claude,
        vec![Ok(vec![claude_snapshot(
            datetime!(2026-06-29 12:00 UTC),
            "window-a",
        )])],
    );
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![Ok(vec![codex_snapshot(
            datetime!(2026-06-30 00:00 UTC),
            "window-a",
        )])],
    );
    let mut daemon = Daemon::new(app_config(auth_path), notifier.clone(), claude, codex);

    daemon
        .run_cycle_at(datetime!(2026-06-29 12:00 UTC))
        .await
        .unwrap();

    assert!(notifier.messages().is_empty());
}

#[tokio::test]
async fn second_cycle_after_reset_sends_one_notification() {
    let (_dir, auth_path) = temp_auth_file();
    write_auth_file(&auth_path, "claude-token", "codex-token");

    let notifier = FakeNotifier::default();
    let claude = FakeProvider::new(
        ProviderKind::Claude,
        vec![
            Ok(vec![claude_snapshot(
                datetime!(2026-06-29 12:00 UTC),
                "window-a",
            )]),
            Ok(vec![claude_snapshot(
                datetime!(2026-06-29 17:00 UTC),
                "window-b",
            )]),
        ],
    );
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![
            Ok(vec![codex_snapshot(
                datetime!(2026-06-30 00:00 UTC),
                "window-a",
            )]),
            Ok(vec![codex_snapshot(
                datetime!(2026-06-30 00:00 UTC),
                "window-a",
            )]),
        ],
    );
    let mut daemon = Daemon::new(app_config(auth_path), notifier.clone(), claude, codex);

    daemon
        .run_cycle_at(datetime!(2026-06-29 12:00 UTC))
        .await
        .unwrap();
    daemon
        .run_cycle_at(datetime!(2026-06-29 17:00 UTC))
        .await
        .unwrap();

    assert_eq!(
        notifier.messages(),
        vec!["Claude Max 5h quota reset at 17:00 UTC".to_string()]
    );
}

#[tokio::test]
async fn non_auth_provider_failure_leaves_other_path_runnable() {
    let (_dir, auth_path) = temp_auth_file();
    write_auth_file(&auth_path, "claude-token", "codex-token");

    let notifier = FakeNotifier::default();
    let claude = FakeProvider::new(
        ProviderKind::Claude,
        vec![
            Err(ProviderRequestError::Other(anyhow!("claude boom"))),
            Err(ProviderRequestError::Other(anyhow!("claude boom"))),
        ],
    );
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![
            Ok(vec![codex_snapshot(
                datetime!(2026-06-30 00:00 UTC),
                "window-a",
            )]),
            Ok(vec![codex_snapshot(
                datetime!(2026-07-07 00:00 UTC),
                "window-b",
            )]),
        ],
    );
    let mut daemon = Daemon::new(
        app_config(auth_path),
        notifier.clone(),
        claude.clone(),
        codex,
    );

    daemon
        .run_cycle_at(datetime!(2026-06-29 12:00 UTC))
        .await
        .unwrap();
    daemon
        .run_cycle_at(datetime!(2026-07-07 00:00 UTC))
        .await
        .unwrap();

    assert_eq!(
        notifier.messages(),
        vec!["Codex Pro 7d quota reset at 00:00 UTC".to_string()]
    );
    assert_eq!(claude.fetch_tokens(), vec!["claude-token", "claude-token"]);
}

#[tokio::test]
async fn auth_failure_triggers_refresh_credentials_once_and_then_succeeds() {
    let (_dir, auth_path) = temp_auth_file();
    write_auth_file(&auth_path, "stale-token", "codex-token");

    let notifier = FakeNotifier::default();
    let refreshed = ProviderCredentials {
        access_token: "fresh-token".into(),
        refresh_token: Some("claude-refresh".into()),
        expires_at: None,
        account_id: Some("claude-account".into()),
        raw_source: HashMap::new(),
    };
    let claude = FakeProvider::new(
        ProviderKind::Claude,
        vec![
            Err(ProviderRequestError::Authentication),
            Ok(vec![claude_snapshot(
                datetime!(2026-06-29 12:00 UTC),
                "window-a",
            )]),
        ],
    )
    .with_refreshed_credentials(refreshed);
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![Ok(vec![codex_snapshot(
            datetime!(2026-06-30 00:00 UTC),
            "window-a",
        )])],
    );
    let mut daemon = Daemon::new(app_config(auth_path), notifier, claude.clone(), codex);

    daemon
        .run_cycle_at(datetime!(2026-06-29 12:00 UTC))
        .await
        .unwrap();

    assert_eq!(claude.refresh_count(), 1);
    assert_eq!(claude.refresh_tokens(), vec!["stale-token"]);
    assert_eq!(claude.fetch_tokens(), vec!["stale-token", "fresh-token"]);
}

#[tokio::test]
async fn auth_file_reload_picks_up_changed_credentials_on_next_cycle() {
    let (_dir, auth_path) = temp_auth_file();
    write_auth_file(&auth_path, "claude-old", "codex-old");

    let notifier = FakeNotifier::default();
    let claude = FakeProvider::new(ProviderKind::Claude, vec![Ok(Vec::new()), Ok(Vec::new())]);
    let codex = FakeProvider::new(ProviderKind::Codex, vec![Ok(Vec::new()), Ok(Vec::new())]);
    let mut daemon = Daemon::new(
        app_config(auth_path.clone()),
        notifier,
        claude.clone(),
        codex.clone(),
    );

    daemon
        .run_cycle_at(datetime!(2026-06-29 12:00 UTC))
        .await
        .unwrap();

    write_auth_file(&auth_path, "claude-new", "codex-new");

    daemon
        .run_cycle_at(datetime!(2026-06-29 12:10 UTC))
        .await
        .unwrap();

    assert_eq!(claude.fetch_tokens(), vec!["claude-old", "claude-new"]);
    assert_eq!(codex.fetch_tokens(), vec!["codex-old", "codex-new"]);
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
