use crate::{
    error::AppResult,
    model::{ProviderKind, QuotaSnapshot, WindowKind, format_remaining},
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[async_trait]
pub trait ResetNotifier: Send + Sync {
    /// Send a free-form text message, used for startup summaries and reset
    /// notifications. Returns the Telegram `message_id` on success so the
    /// caller can later edit that same message in place.
    async fn notify_text(&self, text: &str) -> AppResult<Option<i64>>;

    /// Edit a previously sent message in place.
    async fn edit_text(&self, message_id: i64, text: &str) -> AppResult<()>;

    /// Delete a previously sent message.
    async fn delete_text(&self, message_id: i64) -> AppResult<()>;
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
    async fn notify_text(&self, text: &str) -> AppResult<Option<i64>> {
        self.send_text(text).await
    }

    async fn edit_text(&self, message_id: i64, text: &str) -> AppResult<()> {
        self.edit_message_text(message_id, text).await
    }

    async fn delete_text(&self, message_id: i64) -> AppResult<()> {
        self.delete_message(message_id).await
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

    pub async fn send_text(&self, text: &str) -> AppResult<Option<i64>> {
        let url = format!("{}/bot{}/sendMessage", self.api_base, self.bot_token);
        let body = SendMessageBody {
            chat_id: self.chat_id.clone(),
            text: text.to_string(),
        };

        let response = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json::<TelegramResponse>()
            .await?;

        Ok(response.result.map(|m| m.message_id))
    }

    pub async fn edit_message_text(&self, message_id: i64, text: &str) -> AppResult<()> {
        let url = format!("{}/bot{}/editMessageText", self.api_base, self.bot_token);
        let body = EditMessageBody {
            chat_id: self.chat_id.clone(),
            message_id,
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

    pub async fn delete_message(&self, message_id: i64) -> AppResult<()> {
        let url = format!("{}/bot{}/deleteMessage", self.api_base, self.bot_token);
        let body = DeleteMessageBody {
            chat_id: self.chat_id.clone(),
            message_id,
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

#[derive(Debug, Serialize)]
struct EditMessageBody {
    chat_id: String,
    message_id: i64,
    text: String,
}

#[derive(Debug, Serialize)]
struct DeleteMessageBody {
    chat_id: String,
    message_id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramResponse {
    result: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    message_id: i64,
}

/// Format a single quota-window snapshot line, e.g. `7d: 27% used (4d 19h)`.
pub fn format_window_line(
    window_kind: WindowKind,
    reset_at: Option<OffsetDateTime>,
    usage: Option<u64>,
    limit: Option<u64>,
    now: OffsetDateTime,
) -> String {
    let label = window_kind.as_str();
    let pct = match (usage, limit) {
        (Some(u), Some(l)) if l > 0 => format!("{}% left", 100 - (u * 100 / l).min(100)),
        _ => "?".to_string(),
    };
    let remaining = format_remaining(reset_at, now);
    format!("{}: {} ({})", label, pct, remaining)
}

/// Build one provider line from its snapshots, e.g.
/// `claude: 7d: 27% used (4d 19h), 5h: 0% used (unknown)`.
pub fn format_provider_line(
    provider: ProviderKind,
    snapshots: &[QuotaSnapshot],
    now: OffsetDateTime,
) -> String {
    let provider_name = provider.as_str();
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
        let soonest = snapshots.iter().filter_map(|s| s.reset_soonest_expiry).min();
        let expiry = match soonest {
            Some(_) => format!(", next expires in {}", format_remaining(soonest, now)),
            None => String::new(),
        };
        parts.push(format!(
            "{} reset{}{}",
            resets_available,
            if resets_available == 1 { "" } else { "s" },
            expiry
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

    let header = if providers.is_none() {
        "🚀 Startup summary"
    } else {
        "🔄 Reset detected"
    };
    format!("{}\n{}", header, lines.join("\n"))
}

/// Build the summary body (provider lines only, no header). Returns an empty
/// string when there are no matching snapshots. Used when the caller wants to
/// supply its own header, e.g. the live-updated status message.
pub fn format_summary_body(
    snapshots: &[QuotaSnapshot],
    providers: Option<&[ProviderKind]>,
    now: OffsetDateTime,
) -> String {
    let candidates: &[ProviderKind] = providers.unwrap_or(&[ProviderKind::Claude, ProviderKind::Codex]);

    candidates
        .iter()
        .filter_map(|&provider| {
            let provider_snapshots: Vec<QuotaSnapshot> = snapshots
                .iter()
                .filter(|s| s.provider == provider)
                .cloned()
                .collect();
            if provider_snapshots.is_empty() {
                None
            } else {
                Some(format_provider_line(provider, &provider_snapshots, now))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
