use crate::{
    error::AppResult,
    model::{ProviderKind, QuotaSnapshot, WindowKind, format_remaining},
};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use time::OffsetDateTime;

#[async_trait]
pub trait ResetNotifier: Send + Sync {
    /// Send a free-form text message, used for startup summaries and reset notifications.
    async fn notify_text(&self, text: &str) -> AppResult<()>;
}

#[derive(Clone, Debug)]
pub struct TelegramClient {
    client: Client,
    bot_token: String,
    chat_id: String,
    api_base: String,
}

#[async_trait]
impl ResetNotifier for TelegramClient {
    async fn notify_text(&self, text: &str) -> AppResult<()> {
        self.send_text(text).await
    }
}

impl TelegramClient {
    pub fn new(bot_token: String, chat_id: String) -> Self {
        Self::with_api_base(
            Client::new(),
            bot_token,
            chat_id,
            "https://api.telegram.org",
        )
    }

    pub fn with_api_base(
        client: Client,
        bot_token: String,
        chat_id: String,
        api_base: impl Into<String>,
    ) -> Self {
        Self {
            client,
            bot_token,
            chat_id,
            api_base: api_base.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn send_text(&self, text: &str) -> AppResult<()> {
        let url = format!("{}/bot{}/sendMessage", self.api_base, self.bot_token);
        let body = SendMessageBody {
            chat_id: self.chat_id.clone(),
            text: text.to_string(),
        };

        self.client
            .post(url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct SendMessageBody {
    chat_id: String,
    text: String,
}

/// Format a single quota-window snapshot line, e.g. `7d: 73% left (4d 19h)`.
pub fn format_window_line(
    window_kind: WindowKind,
    reset_at: Option<OffsetDateTime>,
    usage: Option<u64>,
    limit: Option<u64>,
    now: OffsetDateTime,
) -> String {
    let label = window_kind.as_str();
    let pct = match (usage, limit) {
        (Some(u), Some(l)) if l > 0 => format!("{}% left", 100 - (u * 100 / l)),
        _ => "?".to_string(),
    };
    let remaining = format_remaining(reset_at, now);
    format!("{}: {} ({})", label, pct, remaining)
}

/// Build one provider line from its snapshots, e.g.
/// `claude: 7d: 73% left (4d 19h), 5h: 100% left (unknown)`.
pub fn format_provider_line(
    provider: ProviderKind,
    snapshots: &[QuotaSnapshot],
    now: OffsetDateTime,
) -> String {
    let provider_name = display_provider(provider);
    let mut parts: Vec<String> = Vec::new();

    for window_kind in [WindowKind::SevenDays, WindowKind::FiveHours] {
        if let Some(s) = snapshots.iter().find(|s| s.window_kind == window_kind) {
            parts.push(format_window_line(
                s.window_kind,
                s.reset_at,
                s.usage,
                s.limit,
                now,
            ));
        }
    }

    let resets_available = snapshots.iter().map(|s| s.resets_available).max().unwrap_or(0);
    if resets_available > 0 {
        parts.push(format!(
            "{} reset{}",
            resets_available,
            if resets_available == 1 { "" } else { "s" }
        ));
    }

    format!("{}: {}", provider_name, parts.join(", "))
}

/// Build the full summary message with optional provider filter.
/// When `providers` is `None`, all providers are included (startup).
/// When `providers` is `Some(...)`, only those providers appear (reset).
pub fn format_summary_message(
    snapshots: &[QuotaSnapshot],
    providers: Option<&[ProviderKind]>,
    now: OffsetDateTime,
) -> String {
    let mut lines: Vec<String> = Vec::new();

    let candidates: &[ProviderKind] = if let Some(filter) = providers {
        filter
    } else {
        &[ProviderKind::Claude, ProviderKind::Codex]
    };

    for &provider in candidates {
        let provider_snapshots: Vec<&QuotaSnapshot> = snapshots
            .iter()
            .filter(|s| s.provider == provider)
            .collect();

        if provider_snapshots.is_empty() {
            continue;
        }

        lines.push(format_provider_line(
            provider,
            &provider_snapshots.into_iter().cloned().collect::<Vec<_>>(),
            now,
        ));
    }

    if lines.is_empty() {
        return String::new();
    }

    lines.join("\n")
}

fn display_provider(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Claude => "claude",
        ProviderKind::Codex => "codex",
    }
}
