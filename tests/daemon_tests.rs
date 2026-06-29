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
    collections::VecDeque,
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
    }
}

fn claude_snapshot(reset_at: OffsetDateTime, window_id: &str, usage: u64) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Claude,
        window_kind: WindowKind::FiveHours,
        window_id: Some(window_id.into()),
        reset_at,
        usage: Some(usage),
        limit: Some(100),
    }
}

fn codex_snapshot(reset_at: OffsetDateTime, window_id: &str, usage: u64) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Codex,
        window_kind: WindowKind::SevenDays,
        window_id: Some(window_id.into()),
        reset_at,
        usage: Some(usage),
        limit: Some(100),
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
        self.sent
            .lock()
            .unwrap()
            .push(format_reset_message(event, event.reset_at));
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
            42,
        )])],
    );
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![Ok(vec![codex_snapshot(
            datetime!(2026-06-30 00:00 UTC),
            "window-a",
            10,
        )])],
    );
    let mut daemon = Daemon::new(app_config(auth_path), notifier.clone(), claude, codex);

    daemon.run_cycle_at(datetime!(2026-06-29 12:00 UTC)).await;

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
                42,
            )]),
            Ok(vec![claude_snapshot(
                datetime!(2026-06-29 17:00 UTC),
                "window-b",
                0,
            )]),
        ],
    );
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![
            Ok(vec![codex_snapshot(
                datetime!(2026-06-30 00:00 UTC),
                "window-a",
                10,
            )]),
            Ok(vec![codex_snapshot(
                datetime!(2026-06-30 00:00 UTC),
                "window-a",
                10,
            )]),
        ],
    );
    let mut daemon = Daemon::new(app_config(auth_path), notifier.clone(), claude, codex);

    daemon.run_cycle_at(datetime!(2026-06-29 12:00 UTC)).await;
    daemon.run_cycle_at(datetime!(2026-06-29 17:00 UTC)).await;

    assert_eq!(
        notifier.messages(),
        vec!["📊 Quota summary\nClaude: 5h 0% used (0m)".to_string()]
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
                33,
            )]),
            Ok(vec![codex_snapshot(
                datetime!(2026-07-07 00:00 UTC),
                "window-b",
                5,
            )]),
        ],
    );
    let mut daemon = Daemon::new(
        app_config(auth_path),
        notifier.clone(),
        claude.clone(),
        codex,
    );

    daemon.run_cycle_at(datetime!(2026-06-29 12:00 UTC)).await;
    daemon.run_cycle_at(datetime!(2026-07-07 00:00 UTC)).await;

    assert_eq!(
        notifier.messages(),
        vec!["📊 Quota summary\nCodex: 7d 5% used (0m)".to_string()]
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
    };
    let claude = FakeProvider::new(
        ProviderKind::Claude,
        vec![
            Err(ProviderRequestError::Authentication),
            Ok(vec![claude_snapshot(
                datetime!(2026-06-29 12:00 UTC),
                "window-a",
                42,
            )]),
        ],
    )
    .with_refreshed_credentials(refreshed);
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![Ok(vec![codex_snapshot(
            datetime!(2026-06-30 00:00 UTC),
            "window-a",
            10,
        )])],
    );
    let mut daemon = Daemon::new(app_config(auth_path), notifier, claude.clone(), codex);

    daemon.run_cycle_at(datetime!(2026-06-29 12:00 UTC)).await;

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

    daemon.run_cycle_at(datetime!(2026-06-29 12:00 UTC)).await;

    write_auth_file(&auth_path, "claude-new", "codex-new");

    daemon.run_cycle_at(datetime!(2026-06-29 12:10 UTC)).await;

    assert_eq!(claude.fetch_tokens(), vec!["claude-old", "claude-new"]);
    assert_eq!(codex.fetch_tokens(), vec!["codex-old", "codex-new"]);
}

#[tokio::test]
async fn scheduled_fired_flag_suppresses_detector_notification() {
    let (_dir, auth_path) = temp_auth_file();
    write_auth_file(&auth_path, "claude-token", "codex-token");

    let notifier = FakeNotifier::default();
    let claude = FakeProvider::new(
        ProviderKind::Claude,
        vec![
            Ok(vec![claude_snapshot(
                datetime!(2026-06-29 12:00 UTC),
                "window-a",
                42,
            )]),
            Ok(vec![claude_snapshot(
                datetime!(2026-06-29 17:00 UTC),
                "window-b",
                0,
            )]),
        ],
    );
    let codex = FakeProvider::new(
        ProviderKind::Codex,
        vec![
            Ok(vec![codex_snapshot(
                datetime!(2026-06-30 00:00 UTC),
                "window-a",
                10,
            )]),
            Ok(vec![codex_snapshot(
                datetime!(2026-06-30 00:00 UTC),
                "window-a",
                10,
            )]),
        ],
    );
    let mut daemon = Daemon::new(app_config(auth_path), notifier.clone(), claude, codex);

    daemon.run_cycle_at(datetime!(2026-06-29 12:00 UTC)).await;

    // Simulate that the scheduler already fired a notification for this window.
    daemon
        .scheduled_fired
        .insert((ProviderKind::Claude, WindowKind::FiveHours));

    daemon.run_cycle_at(datetime!(2026-06-29 17:00 UTC)).await;

    // The detector normally would fire here, but it's suppressed.
    assert!(
        notifier.messages().is_empty(),
        "expected no notifications when scheduled_fired suppresses detector"
    );
}

#[tokio::test]
async fn claude_adapter_parses_five_hour_window() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oauth/usage")
            .header("authorization", "Bearer token")
            .header("anthropic-beta", "claude-code-20250219,oauth-2025-04-20");
        then.status(200)
            .header("content-type", "application/json")
            .body(support::claude_usage_response());
    });

    let provider = ClaudeProvider::new(server.url(""));
    let snapshots = provider.fetch_snapshots(&credentials()).await.unwrap();

    mock.assert();
    // Claude returns 2 windows (5h, 7d) from the real endpoint
    assert_eq!(snapshots.len(), 2);

    let five_hour = snapshots
        .iter()
        .find(|s| s.window_kind == WindowKind::FiveHours)
        .unwrap();
    assert_eq!(five_hour.provider, ProviderKind::Claude);
    assert_eq!(five_hour.usage, Some(12));
    assert_eq!(five_hour.limit, Some(100));

    let seven_day = snapshots
        .iter()
        .find(|s| s.window_kind == WindowKind::SevenDays)
        .unwrap();
    assert_eq!(seven_day.provider, ProviderKind::Claude);
    assert_eq!(seven_day.usage, Some(33));
}

#[tokio::test]
async fn codex_adapter_parses_seven_day_window() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/backend-api/wham/usage")
            .header("authorization", "Bearer token")
            .header("ChatGPT-Account-Id", "codex-account");
        then.status(200)
            .header("content-type", "application/json")
            .body(support::codex_usage_response());
    });

    let mut creds = credentials();
    creds.account_id = Some("codex-account".into());

    let provider = CodexProvider::new(server.url(""));
    let snapshots = provider.fetch_snapshots(&creds).await.unwrap();

    mock.assert();
    // Codex returns 2 windows (5h, 7d) from the real endpoint
    assert_eq!(snapshots.len(), 2);

    let five_hour = snapshots
        .iter()
        .find(|s| s.window_kind == WindowKind::FiveHours)
        .unwrap();
    assert_eq!(five_hour.provider, ProviderKind::Codex);
    assert_eq!(five_hour.usage, Some(6));

    let seven_day = snapshots
        .iter()
        .find(|s| s.window_kind == WindowKind::SevenDays)
        .unwrap();
    assert_eq!(seven_day.provider, ProviderKind::Codex);
    assert_eq!(seven_day.usage, Some(25));
    assert_eq!(seven_day.limit, Some(100));
}

#[tokio::test]
async fn unauthorized_provider_response_maps_to_authentication_error() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET).path("/api/oauth/usage");
        then.status(401);
    });

    let provider = ClaudeProvider::new(server.url(""));
    let error = provider.fetch_snapshots(&credentials()).await.unwrap_err();

    mock.assert();
    assert!(matches!(error, ProviderRequestError::Authentication));
}
