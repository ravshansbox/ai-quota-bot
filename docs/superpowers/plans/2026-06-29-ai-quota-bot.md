# AI Quota Bot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust daemon that reads Claude and Codex credentials from `~/.pi/agent/auth.json`, polls provider usage endpoints every 10 minutes, refreshes tokens when needed, detects 5-hour and 7-day quota resets in memory, and sends Telegram notifications only for actual reset events.

**Architecture:** The app is a Tokio-based daemon with narrow modules for config, auth parsing, token refresh, provider adapters, reset detection, Telegram delivery, and the polling loop. Core behavior is verified with unit tests for parsing, reset detection, and refresh flow plus mocked integration tests for provider requests and the daemon cycle.

**Tech Stack:** Rust 2024, Tokio, Reqwest, Serde, Serde JSON, Time, ThisError or Anyhow, Tracing, Wiremock or HTTP mock tooling

---

## Planned File Structure

### New or modified production files
- Modify: `Cargo.toml` for runtime and test dependencies
- Modify: `src/main.rs` to bootstrap config, logging, and daemon runtime
- Create: `src/lib.rs` to expose reusable modules to integration tests
- Create: `src/config.rs` for environment and path loading
- Create: `src/model.rs` for `ProviderKind`, `WindowKind`, `QuotaSnapshot`, `ResetEvent`, and provider-neutral auth types
- Create: `src/error.rs` for shared app error types
- Create: `src/auth.rs` for reading and parsing `~/.pi/agent/auth.json`
- Create: `src/auth_refresh.rs` for refresh policy and retry orchestration
- Create: `src/detector.rs` for in-memory reset detection
- Create: `src/telegram.rs` for Telegram HTTP client and message formatting
- Create: `src/daemon.rs` for the polling loop and graceful shutdown
- Create: `src/providers/mod.rs` for shared provider traits and adapter exports
- Create: `src/providers/claude.rs` for Claude usage and refresh integration
- Create: `src/providers/codex.rs` for Codex usage and refresh integration

### New test files
- Create: `tests/config_tests.rs`
- Create: `tests/auth_tests.rs`
- Create: `tests/detector_tests.rs`
- Create: `tests/auth_refresh_tests.rs`
- Create: `tests/telegram_tests.rs`
- Create: `tests/daemon_tests.rs`
- Create: `tests/fixtures/auth/sample_auth.json`
- Create: `tests/support/mod.rs`

### Module responsibility boundaries
- `config.rs` owns env parsing and defaults only
- `model.rs` owns cross-module data types only
- `auth.rs` owns file reading and provider credential extraction only
- `auth_refresh.rs` owns token-expiry policy and request retry orchestration only
- `providers/*.rs` own provider-specific HTTP and response parsing only
- `detector.rs` owns state transition logic only
- `telegram.rs` owns outbound notification formatting and delivery only
- `daemon.rs` owns scheduling, lifecycle, and composition only

### Task 1: Create project skeleton and dependency baseline

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/main.rs`
- Create: `src/lib.rs`
- Create: `src/error.rs`
- Create: `src/model.rs`

- [ ] **Step 1: Write the failing compile-level smoke test**

```rust
// tests/config_tests.rs
use ai_quota_bot::model::{ProviderKind, WindowKind};

#[test]
fn model_enums_are_exposed() {
    assert_eq!(ProviderKind::Claude.as_str(), "claude");
    assert_eq!(WindowKind::FiveHours.as_str(), "5h");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test model_enums_are_exposed --test config_tests -q`
Expected: FAIL with unresolved crate items such as `could not find model in ai_quota_bot`

- [ ] **Step 3: Add dependencies and minimal library skeleton**

```toml
# Cargo.toml
[package]
name = "ai-quota-bot"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1"
async-trait = "0.1"
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
time = { version = "0.3", features = ["formatting", "macros", "parsing", "serde"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "signal", "time"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

[dev-dependencies]
httpmock = "0.7"
tempfile = "3"
```

```rust
// src/lib.rs
pub mod error;
pub mod model;
```

```rust
// src/model.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Claude,
    Codex,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowKind {
    FiveHours,
    SevenDays,
}

impl WindowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveHours => "5h",
            Self::SevenDays => "7d",
        }
    }
}
```

```rust
// src/error.rs
pub type AppResult<T> = anyhow::Result<T>;
```

```rust
// src/main.rs
fn main() {
    println!("ai-quota-bot bootstrap pending");
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test model_enums_are_exposed --test config_tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/lib.rs src/model.rs src/error.rs src/main.rs tests/config_tests.rs
git commit -m "build: add ai-quota-bot crate skeleton"
```

### Task 2: Implement config loading with defaults and validation

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs`
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write the failing config tests**

```rust
// tests/config_tests.rs
use ai_quota_bot::config::AppConfig;
use std::path::PathBuf;

#[test]
fn config_uses_defaults_when_optional_values_missing() {
    std::env::remove_var("AI_QUOTA_AUTH_PATH");
    std::env::remove_var("AI_QUOTA_POLL_INTERVAL_SECS");
    std::env::set_var("TELEGRAM_BOT_TOKEN", "bot-token");
    std::env::set_var("TELEGRAM_CHAT_ID", "1234");

    let config = AppConfig::from_env().unwrap();
    assert_eq!(config.telegram_chat_id, "1234");
    assert_eq!(config.poll_interval_secs, 600);
    assert!(config.auth_path.ends_with(PathBuf::from(".pi/agent/auth.json")));
}

#[test]
fn config_errors_when_required_telegram_values_missing() {
    std::env::remove_var("TELEGRAM_BOT_TOKEN");
    std::env::remove_var("TELEGRAM_CHAT_ID");

    let error = AppConfig::from_env().unwrap_err().to_string();
    assert!(error.contains("TELEGRAM_BOT_TOKEN"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_tests -q`
Expected: FAIL with unresolved import `ai_quota_bot::config`

- [ ] **Step 3: Implement minimal config loader**

```rust
// src/config.rs
use crate::error::AppResult;
use anyhow::{anyhow, Context};
use std::{env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub telegram_bot_token: String,
    pub telegram_chat_id: String,
    pub auth_path: PathBuf,
    pub poll_interval_secs: u64,
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        let telegram_bot_token = env::var("TELEGRAM_BOT_TOKEN")
            .map_err(|_| anyhow!("missing TELEGRAM_BOT_TOKEN"))?;
        let telegram_chat_id = env::var("TELEGRAM_CHAT_ID")
            .map_err(|_| anyhow!("missing TELEGRAM_CHAT_ID"))?;

        let auth_path = env::var("AI_QUOTA_AUTH_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = env::var("HOME").unwrap_or_else(|_| String::from("~"));
                PathBuf::from(home).join(".pi/agent/auth.json")
            });

        let poll_interval_secs = env::var("AI_QUOTA_POLL_INTERVAL_SECS")
            .ok()
            .map(|raw| raw.parse::<u64>().context("invalid AI_QUOTA_POLL_INTERVAL_SECS"))
            .transpose()?
            .unwrap_or(600);

        Ok(Self {
            telegram_bot_token,
            telegram_chat_id,
            auth_path,
            poll_interval_secs,
        })
    }
}
```

```rust
// src/lib.rs
pub mod config;
pub mod error;
pub mod model;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test config_tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/lib.rs tests/config_tests.rs
git commit -m "feat(config): load daemon configuration from environment"
```

### Task 3: Implement provider-neutral models and auth file parsing

**Files:**
- Modify: `src/model.rs`
- Create: `src/auth.rs`
- Modify: `src/lib.rs`
- Test: `tests/auth_tests.rs`
- Create: `tests/fixtures/auth/sample_auth.json`

- [ ] **Step 1: Write the failing auth parsing tests**

```rust
// tests/auth_tests.rs
use ai_quota_bot::{auth::load_credentials_map, model::ProviderKind};
use std::path::Path;

#[test]
fn auth_loader_extracts_claude_and_codex_credentials() {
    let creds = load_credentials_map(Path::new("tests/fixtures/auth/sample_auth.json")).unwrap();
    assert_eq!(creds[&ProviderKind::Claude].access_token, "claude-access");
    assert_eq!(creds[&ProviderKind::Codex].refresh_token.as_deref(), Some("codex-refresh"));
}
```

```json
// tests/fixtures/auth/sample_auth.json
{
  "claude": {
    "access_token": "claude-access",
    "refresh_token": "claude-refresh",
    "expires_at": "2026-06-29T14:00:00Z"
  },
  "codex": {
    "access_token": "codex-access",
    "refresh_token": "codex-refresh",
    "expires_at": "2026-06-29T14:30:00Z"
  }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test auth_loader_extracts_claude_and_codex_credentials --test auth_tests -q`
Expected: FAIL with unresolved import `ai_quota_bot::auth`

- [ ] **Step 3: Implement auth models and parser**

```rust
// src/model.rs
use std::collections::HashMap;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub account_id: Option<String>,
    pub raw_source: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaSnapshot {
    pub provider: ProviderKind,
    pub plan: String,
    pub window_kind: WindowKind,
    pub window_id: Option<String>,
    pub reset_at: OffsetDateTime,
    pub usage: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetEvent {
    pub provider: ProviderKind,
    pub plan: String,
    pub window_kind: WindowKind,
    pub reset_at: OffsetDateTime,
    pub previous_window_id: Option<String>,
    pub current_window_id: Option<String>,
}
```

```rust
// src/auth.rs
use crate::{error::AppResult, model::{ProviderCredentials, ProviderKind}};
use anyhow::{anyhow, Context};
use serde::Deserialize;
use std::{collections::HashMap, fs, path::Path};
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
struct RawAuthFile {
    claude: Option<RawProviderAuth>,
    codex: Option<RawProviderAuth>,
}

#[derive(Debug, Deserialize)]
struct RawProviderAuth {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<String>,
    account_id: Option<String>,
}

pub fn load_credentials_map(path: &Path) -> AppResult<HashMap<ProviderKind, ProviderCredentials>> {
    let raw = fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: RawAuthFile = serde_json::from_str(&raw).context("invalid auth json")?;

    let mut out = HashMap::new();
    out.insert(ProviderKind::Claude, convert(parsed.claude.ok_or_else(|| anyhow!("missing claude credentials"))?)?);
    out.insert(ProviderKind::Codex, convert(parsed.codex.ok_or_else(|| anyhow!("missing codex credentials"))?)?);
    Ok(out)
}

fn convert(raw: RawProviderAuth) -> AppResult<ProviderCredentials> {
    let expires_at = raw.expires_at
        .map(|value| OffsetDateTime::parse(&value, &time::format_description::well_known::Rfc3339))
        .transpose()
        .context("invalid expires_at")?;

    Ok(ProviderCredentials {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token,
        expires_at,
        account_id: raw.account_id,
        raw_source: HashMap::new(),
    })
}
```

```rust
// src/lib.rs
pub mod auth;
pub mod config;
pub mod error;
pub mod model;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test auth_tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/model.rs src/auth.rs src/lib.rs tests/auth_tests.rs tests/fixtures/auth/sample_auth.json
git commit -m "feat(auth): parse provider credentials from auth json"
```

### Task 4: Implement reset detection logic with startup suppression

**Files:**
- Create: `src/detector.rs`
- Modify: `src/lib.rs`
- Test: `tests/detector_tests.rs`

- [ ] **Step 1: Write the failing detector tests**

```rust
// tests/detector_tests.rs
use ai_quota_bot::{detector::ResetDetector, model::{ProviderKind, QuotaSnapshot, WindowKind}};
use time::datetime;

fn snapshot(reset_at: time::OffsetDateTime, window_id: &str) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Claude,
        plan: "max".into(),
        window_kind: WindowKind::FiveHours,
        window_id: Some(window_id.into()),
        reset_at,
        usage: Some(42),
        limit: Some(100),
    }
}

#[test]
fn first_poll_initializes_without_alert() {
    let mut detector = ResetDetector::default();
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a")]);
    assert!(events.is_empty());
}

#[test]
fn later_reset_timestamp_emits_event() {
    let mut detector = ResetDetector::default();
    detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a")]);
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 17:00 UTC), "b")]);
    assert_eq!(events.len(), 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test detector_tests -q`
Expected: FAIL with unresolved import `ai_quota_bot::detector`

- [ ] **Step 3: Implement detector state machine**

```rust
// src/detector.rs
use crate::model::{ProviderKind, QuotaSnapshot, ResetEvent, WindowKind};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SnapshotKey {
    provider: ProviderKind,
    plan: String,
    window_kind: WindowKind,
}

#[derive(Default)]
pub struct ResetDetector {
    previous: HashMap<SnapshotKey, QuotaSnapshot>,
    initialized: bool,
}

impl ResetDetector {
    pub fn detect(&mut self, current: Vec<QuotaSnapshot>) -> Vec<ResetEvent> {
        let mut events = Vec::new();
        let mut next = HashMap::new();

        for snapshot in current {
            let key = SnapshotKey {
                provider: snapshot.provider,
                plan: snapshot.plan.clone(),
                window_kind: snapshot.window_kind,
            };

            if self.initialized {
                if let Some(previous) = self.previous.get(&key) {
                    let reset_advanced = snapshot.reset_at > previous.reset_at;
                    let window_changed = snapshot.window_id != previous.window_id;
                    if reset_advanced || window_changed {
                        events.push(ResetEvent {
                            provider: snapshot.provider,
                            plan: snapshot.plan.clone(),
                            window_kind: snapshot.window_kind,
                            reset_at: snapshot.reset_at,
                            previous_window_id: previous.window_id.clone(),
                            current_window_id: snapshot.window_id.clone(),
                        });
                    }
                }
            }

            next.insert(key, snapshot);
        }

        self.previous = next;
        self.initialized = true;
        events
    }
}
```

```rust
// src/lib.rs
pub mod auth;
pub mod config;
pub mod detector;
pub mod error;
pub mod model;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test detector_tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/detector.rs src/lib.rs tests/detector_tests.rs
git commit -m "feat(detector): detect quota reset transitions in memory"
```

### Task 5: Implement Telegram client and message formatting

**Files:**
- Create: `src/telegram.rs`
- Modify: `src/lib.rs`
- Test: `tests/telegram_tests.rs`

- [ ] **Step 1: Write the failing Telegram tests**

```rust
// tests/telegram_tests.rs
use ai_quota_bot::{model::{ProviderKind, ResetEvent, WindowKind}, telegram::format_reset_message};
use time::datetime;

#[test]
fn telegram_message_matches_expected_format() {
    let event = ResetEvent {
        provider: ProviderKind::Claude,
        plan: "max".into(),
        window_kind: WindowKind::FiveHours,
        reset_at: datetime!(2026-06-29 12:00 UTC),
        previous_window_id: Some("old".into()),
        current_window_id: Some("new".into()),
    };

    assert_eq!(format_reset_message(&event), "Claude Max 5h quota reset at 12:00 UTC");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test telegram_tests -q`
Expected: FAIL with unresolved import `ai_quota_bot::telegram`

- [ ] **Step 3: Implement formatting and HTTP send wrapper**

```rust
// src/telegram.rs
use crate::{error::AppResult, model::{ProviderKind, ResetEvent, WindowKind}};
use reqwest::Client;
use serde::Serialize;

#[derive(Clone)]
pub struct TelegramClient {
    client: Client,
    bot_token: String,
    chat_id: String,
}

impl TelegramClient {
    pub fn new(bot_token: String, chat_id: String) -> Self {
        Self { client: Client::new(), bot_token, chat_id }
    }

    pub async fn send_reset(&self, event: &ResetEvent) -> AppResult<()> {
        let url = format!("https://api.telegram.org/bot{}/sendMessage", self.bot_token);
        let body = SendMessageBody {
            chat_id: self.chat_id.clone(),
            text: format_reset_message(event),
        };
        self.client.post(url).json(&body).send().await?.error_for_status()?;
        Ok(())
    }
}

#[derive(Serialize)]
struct SendMessageBody {
    chat_id: String,
    text: String,
}

pub fn format_reset_message(event: &ResetEvent) -> String {
    format!(
        "{} {} {} quota reset at {} UTC",
        display_provider(event.provider),
        capitalize(&event.plan),
        display_window(event.window_kind),
        event.reset_at.format(&time::macros::format_description!("[hour]:[minute]")).unwrap()
    )
}

fn display_provider(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Claude => "Claude",
        ProviderKind::Codex => "Codex",
    }
}

fn display_window(window: WindowKind) -> &'static str {
    match window {
        WindowKind::FiveHours => "5h",
        WindowKind::SevenDays => "7d",
    }
}

fn capitalize(plan: &str) -> String {
    let mut chars = plan.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test telegram_tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/telegram.rs src/lib.rs tests/telegram_tests.rs
git commit -m "feat(telegram): format and send reset notifications"
```

### Task 6: Implement refresh policy and request retry orchestration

**Files:**
- Create: `src/auth_refresh.rs`
- Modify: `src/providers/mod.rs`
- Modify: `src/lib.rs`
- Test: `tests/auth_refresh_tests.rs`

- [ ] **Step 1: Write the failing refresh tests**

```rust
// tests/auth_refresh_tests.rs
use ai_quota_bot::{auth_refresh::should_refresh, model::ProviderCredentials};
use std::collections::HashMap;
use time::{datetime, Duration};

#[test]
fn refresh_is_required_when_expiry_is_within_five_minutes() {
    let creds = ProviderCredentials {
        access_token: "token".into(),
        refresh_token: Some("refresh".into()),
        expires_at: Some(datetime!(2026-06-29 12:05 UTC)),
        account_id: None,
        raw_source: HashMap::new(),
    };

    assert!(should_refresh(&creds, datetime!(2026-06-29 12:01 UTC), Duration::minutes(5)));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test auth_refresh_tests -q`
Expected: FAIL with unresolved import `ai_quota_bot::auth_refresh`

- [ ] **Step 3: Implement refresh policy helpers and provider traits**

```rust
// src/providers/mod.rs
use crate::{error::AppResult, model::{ProviderCredentials, ProviderKind, QuotaSnapshot}};
use async_trait::async_trait;

pub mod claude;
pub mod codex;

#[derive(Debug, thiserror::Error)]
pub enum ProviderRequestError {
    #[error("authentication failed")]
    Authentication,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[async_trait]
pub trait QuotaProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    async fn fetch_snapshots(&self, creds: &ProviderCredentials) -> Result<Vec<QuotaSnapshot>, ProviderRequestError>;
    async fn refresh_credentials(&self, creds: &ProviderCredentials) -> AppResult<ProviderCredentials>;
}
```

```rust
// src/auth_refresh.rs
use crate::{error::AppResult, model::ProviderCredentials, providers::{ProviderRequestError, QuotaProvider}};
use time::{Duration, OffsetDateTime};

pub fn should_refresh(creds: &ProviderCredentials, now: OffsetDateTime, leeway: Duration) -> bool {
    match creds.expires_at {
        Some(expires_at) => expires_at <= now + leeway,
        None => false,
    }
}

pub async fn fetch_with_refresh<P: QuotaProvider>(
    provider: &P,
    creds: &ProviderCredentials,
    now: OffsetDateTime,
) -> AppResult<(ProviderCredentials, Vec<crate::model::QuotaSnapshot>)> {
    let active_creds = if should_refresh(creds, now, Duration::minutes(5)) {
        provider.refresh_credentials(creds).await?
    } else {
        creds.clone()
    };

    match provider.fetch_snapshots(&active_creds).await {
        Ok(snapshots) => Ok((active_creds, snapshots)),
        Err(ProviderRequestError::Authentication) => {
            let refreshed = provider.refresh_credentials(&active_creds).await?;
            let snapshots = provider
                .fetch_snapshots(&refreshed)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            Ok((refreshed, snapshots))
        }
        Err(ProviderRequestError::Other(err)) => Err(err),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test auth_refresh_tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/auth_refresh.rs src/providers/mod.rs src/lib.rs tests/auth_refresh_tests.rs Cargo.toml
git commit -m "feat(auth): add token refresh policy and retry flow"
```

### Task 7: Implement Claude and Codex provider adapters behind a shared trait

**Files:**
- Create: `src/providers/claude.rs`
- Create: `src/providers/codex.rs`
- Modify: `src/providers/mod.rs`
- Modify: `src/model.rs`
- Test: `tests/support/mod.rs`
- Test: `tests/daemon_tests.rs`

- [ ] **Step 1: Write the failing provider normalization tests**

```rust
// tests/daemon_tests.rs
use ai_quota_bot::model::{ProviderKind, WindowKind};

#[test]
fn provider_window_kinds_cover_supported_reset_windows() {
    assert_eq!(ProviderKind::Claude.as_str(), "claude");
    assert_eq!(WindowKind::SevenDays.as_str(), "7d");
}
```

- [ ] **Step 2: Run test to verify it fails for missing adapter wiring**

Run: `cargo test provider_window_kinds_cover_supported_reset_windows --test daemon_tests -q`
Expected: FAIL once adapter modules are referenced but not implemented

- [ ] **Step 3: Implement minimal adapter shapes with normalized parsing points**

```rust
// src/providers/claude.rs
use crate::{error::AppResult, model::{ProviderCredentials, ProviderKind, QuotaSnapshot, WindowKind}, providers::{ProviderRequestError, QuotaProvider}};
use async_trait::async_trait;
use reqwest::Client;
use time::OffsetDateTime;

pub struct ClaudeProvider {
    client: Client,
    base_url: String,
}

impl ClaudeProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { client: Client::new(), base_url: base_url.into() }
    }
}

#[async_trait]
impl QuotaProvider for ClaudeProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Claude
    }

    async fn fetch_snapshots(&self, _creds: &ProviderCredentials) -> Result<Vec<QuotaSnapshot>, ProviderRequestError> {
        let _ = &self.client;
        let _ = &self.base_url;
        Ok(vec![QuotaSnapshot {
            provider: ProviderKind::Claude,
            plan: "max".into(),
            window_kind: WindowKind::FiveHours,
            window_id: Some("placeholder-window".into()),
            reset_at: OffsetDateTime::now_utc(),
            usage: None,
            limit: None,
        }])
    }

    async fn refresh_credentials(&self, creds: &ProviderCredentials) -> AppResult<ProviderCredentials> {
        Ok(creds.clone())
    }
}
```

```rust
// src/providers/codex.rs
use crate::{error::AppResult, model::{ProviderCredentials, ProviderKind, QuotaSnapshot, WindowKind}, providers::{ProviderRequestError, QuotaProvider}};
use async_trait::async_trait;
use reqwest::Client;
use time::OffsetDateTime;

pub struct CodexProvider {
    client: Client,
    base_url: String,
}

impl CodexProvider {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self { client: Client::new(), base_url: base_url.into() }
    }
}

#[async_trait]
impl QuotaProvider for CodexProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Codex
    }

    async fn fetch_snapshots(&self, _creds: &ProviderCredentials) -> Result<Vec<QuotaSnapshot>, ProviderRequestError> {
        let _ = &self.client;
        let _ = &self.base_url;
        Ok(vec![QuotaSnapshot {
            provider: ProviderKind::Codex,
            plan: "pro".into(),
            window_kind: WindowKind::SevenDays,
            window_id: Some("placeholder-window".into()),
            reset_at: OffsetDateTime::now_utc(),
            usage: None,
            limit: None,
        }])
    }

    async fn refresh_credentials(&self, creds: &ProviderCredentials) -> AppResult<ProviderCredentials> {
        Ok(creds.clone())
    }
}
```

- [ ] **Step 4: Replace placeholders with real HTTP parsing backed by mocks**

```rust
// tests/support/mod.rs
pub fn claude_usage_response(reset_at: &str) -> String {
    format!(r#"{{"plan":"max","window_kind":"5h","window_id":"claude-window","reset_at":"{}","usage":12,"limit":100}}"#, reset_at)
}

pub fn codex_usage_response(reset_at: &str) -> String {
    format!(r#"{{"plan":"pro","window_kind":"7d","window_id":"codex-window","reset_at":"{}","usage":3,"limit":50}}"#, reset_at)
}
```

Use `httpmock` in `tests/daemon_tests.rs` to verify:
- Claude adapter parses a 5h reset response into `WindowKind::FiveHours`
- Codex adapter parses a 7d reset response into `WindowKind::SevenDays`
- 401 responses map to `ProviderRequestError::Authentication`

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test daemon_tests -q`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/providers/claude.rs src/providers/codex.rs src/providers/mod.rs src/model.rs tests/support/mod.rs tests/daemon_tests.rs
git commit -m "feat(providers): add claude and codex quota adapters"
```

### Task 8: Implement daemon orchestration and graceful polling loop

**Files:**
- Create: `src/daemon.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Test: `tests/daemon_tests.rs`

- [ ] **Step 1: Write the failing daemon orchestration test**

```rust
// tests/daemon_tests.rs
use ai_quota_bot::detector::ResetDetector;

#[test]
fn detector_can_be_embedded_in_daemon_state() {
    let _detector = ResetDetector::default();
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test detector_can_be_embedded_in_daemon_state --test daemon_tests -q`
Expected: FAIL with unresolved import `ai_quota_bot::daemon`

- [ ] **Step 3: Implement daemon state and one-cycle execution**

```rust
// src/daemon.rs
use crate::{
    auth::load_credentials_map,
    auth_refresh::fetch_with_refresh,
    config::AppConfig,
    detector::ResetDetector,
    error::AppResult,
    providers::QuotaProvider,
    telegram::TelegramClient,
};
use time::OffsetDateTime;

pub struct Daemon<P1, P2> {
    pub config: AppConfig,
    pub telegram: TelegramClient,
    pub claude: P1,
    pub codex: P2,
    pub detector: ResetDetector,
}

impl<P1, P2> Daemon<P1, P2>
where
    P1: QuotaProvider,
    P2: QuotaProvider,
{
    pub async fn run_cycle(&mut self) -> AppResult<()> {
        let mut creds = load_credentials_map(&self.config.auth_path)?;
        let now = OffsetDateTime::now_utc();
        let (_, claude) = fetch_with_refresh(&self.claude, &creds.remove(&self.claude.kind()).unwrap(), now).await?;
        let (_, codex) = fetch_with_refresh(&self.codex, &creds.remove(&self.codex.kind()).unwrap(), now).await?;

        let mut all = Vec::new();
        all.extend(claude);
        all.extend(codex);

        for event in self.detector.detect(all) {
            self.telegram.send_reset(&event).await?;
        }
        Ok(())
    }
}
```

```rust
// src/main.rs
use ai_quota_bot::{config::AppConfig, daemon::Daemon, detector::ResetDetector, telegram::TelegramClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = AppConfig::from_env()?;
    let telegram = TelegramClient::new(config.telegram_bot_token.clone(), config.telegram_chat_id.clone());

    let _ = (config, telegram, ResetDetector::default());
    Ok(())
}
```

- [ ] **Step 4: Extend to full loop with shutdown handling**

```rust
// src/daemon.rs
impl<P1, P2> Daemon<P1, P2>
where
    P1: QuotaProvider,
    P2: QuotaProvider,
{
    pub async fn run_forever(&mut self) -> AppResult<()> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(self.config.poll_interval_secs));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(error) = self.run_cycle().await {
                        tracing::warn!(error = %error, "poll cycle failed");
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("shutdown signal received");
                    return Ok(());
                }
            }
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test daemon_tests -q`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/daemon.rs src/main.rs src/lib.rs tests/daemon_tests.rs
git commit -m "feat(daemon): orchestrate polling and reset notifications"
```

### Task 9: Add end-to-end mocked behavior tests and startup-noise protections

**Files:**
- Modify: `tests/daemon_tests.rs`
- Modify: `tests/auth_refresh_tests.rs`
- Modify: `tests/telegram_tests.rs`

- [ ] **Step 1: Write failing integration scenarios**

```rust
// tests/daemon_tests.rs
use ai_quota_bot::{detector::ResetDetector, model::{ProviderKind, QuotaSnapshot, WindowKind}};
use time::datetime;

fn claude_snapshot(reset_at: time::OffsetDateTime, window_id: &str) -> QuotaSnapshot {
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

#[tokio::test]
async fn first_successful_cycle_sends_no_notifications() {
    let mut detector = ResetDetector::default();
    let first_events = detector.detect(vec![claude_snapshot(datetime!(2026-06-29 12:00 UTC), "window-a")]);
    assert!(first_events.is_empty());
}

#[tokio::test]
async fn second_cycle_after_reset_sends_one_notification() {
    let mut detector = ResetDetector::default();
    detector.detect(vec![claude_snapshot(datetime!(2026-06-29 12:00 UTC), "window-a")]);

    let second_events = detector.detect(vec![claude_snapshot(datetime!(2026-06-29 17:00 UTC), "window-b")]);
    assert_eq!(second_events.len(), 1);
    assert_eq!(second_events[0].provider, ProviderKind::Claude);
}
```

- [ ] **Step 2: Run tests to verify they fail for missing fake wiring**

Run: `cargo test --test daemon_tests -q`
Expected: FAIL because fake provider and fake Telegram helpers are not implemented yet

- [ ] **Step 3: Implement test doubles and assertions**

Create simple in-test fakes and drive them through `run_cycle()`:

```rust
#[derive(Clone, Default)]
struct FakeTelegram {
    sent: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl FakeTelegram {
    async fn send(&self, text: String) {
        self.sent.lock().unwrap().push(text);
    }
}
```

Also add a `FakeProvider` that implements `QuotaProvider` with queued `Vec<QuotaSnapshot>` responses and an optional `ProviderRequestError::Authentication` on the first call. Verify all of these cases with explicit assertions:
- first cycle sends nothing and only initializes detector state
- second cycle after a reset sends exactly one message with `format_reset_message()` output
- non-auth provider failure leaves the other provider path runnable and logged
- auth failure triggers `refresh_credentials()` exactly once and then succeeds
- auth file reload picks up changed credentials on the next cycle by rewriting a temp auth file between cycles

- [ ] **Step 4: Run focused tests to verify they pass**

Run: `cargo test first_successful_cycle_sends_no_notifications --test daemon_tests -q && cargo test second_cycle_after_reset_sends_one_notification --test daemon_tests -q`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add tests/daemon_tests.rs tests/auth_refresh_tests.rs tests/telegram_tests.rs
git commit -m "test: cover daemon reset flow and refresh edge cases"
```

### Task 10: Final verification and operator docs

**Files:**
- Modify: `src/main.rs`
- Modify: `README.md` if present, otherwise create `README.md`

- [ ] **Step 1: Write the failing operator-doc expectation**

```markdown
# README excerpt expectation
- required env vars
- auth file location
- how to run the daemon
- note about in-memory state and missed resets while offline
```

- [ ] **Step 2: Run the full test suite before docs and final polish**

Run: `cargo test`
Expected: PASS or reveal remaining implementation gaps to fix before documenting completion

- [ ] **Step 3: Add runtime usage docs and polish startup**

```markdown
# README.md
## Configuration
- TELEGRAM_BOT_TOKEN
- TELEGRAM_CHAT_ID
- AI_QUOTA_AUTH_PATH
- AI_QUOTA_POLL_INTERVAL_SECS

## Running
cargo run

## Behavior notes
- polls every 10 minutes by default
- keeps reset detection state only in memory
- may miss reset notifications while offline
```

Ensure `main.rs` logs startup, poll interval, and auth path without leaking secrets.

- [ ] **Step 4: Run final verification**

Run: `cargo fmt --check && cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/main.rs README.md
git commit -m "docs: add runbook and polish daemon startup"
```

## Spec Coverage Check

- daemon runtime: Tasks 2, 8, and 10
- auth file parsing: Task 3
- direct provider polling: Task 7
- token refresh: Task 6 and Task 9
- reset detection: Task 4 and Task 9
- Telegram reset-only notifications: Task 5 and Task 9
- no disk persistence: enforced by Tasks 4, 8, and 10 docs
- tests for parsing, reset detection, refresh, and daemon behavior: Tasks 2 through 9

## Implementation Notes

- Prefer `run_cycle()` as the main test seam and keep `run_forever()` thin.
- Keep provider-specific response structs private to each adapter file.
- Do not print tokens in logs or test failure messages.
- If real provider payloads differ from the draft fixtures, update adapter parsing only and keep the common `QuotaSnapshot` model stable.
- If `std::env::set_var` behavior changes under Edition 2024 safety rules, wrap env-mutating tests with a serial strategy or refactor config parsing to accept an injected map in follow-up commits.
