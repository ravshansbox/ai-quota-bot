use crate::{
    error::AppResult,
    model::{ProviderKind, ResetEvent, WindowKind},
};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use time::OffsetDateTime;

#[async_trait]
pub trait ResetNotifier: Send + Sync {
    async fn notify_reset(&self, event: &ResetEvent) -> AppResult<()>;

    /// Send a free-form text message, used for startup summaries etc.
    /// Default implementation is a no-op so test fakes don't need to override it.
    async fn notify_text(&self, _text: &str) -> AppResult<()> {
        Ok(())
    }
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
    async fn notify_reset(&self, event: &ResetEvent) -> AppResult<()> {
        self.send_reset(event).await
    }

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

    pub async fn send_reset(&self, event: &ResetEvent) -> AppResult<()> {
        self.send_text(&format_reset_message(event, OffsetDateTime::now_utc()))
            .await
    }

    async fn send_text(&self, text: &str) -> AppResult<()> {
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

pub fn format_reset_message(event: &ResetEvent, now: OffsetDateTime) -> String {
    let dur = if event.reset_at > now {
        event.reset_at - now
    } else {
        return format!(
            "{} {} remaining: 0m",
            display_provider(event.provider),
            display_window(event.window_kind),
        );
    };

    let remaining = match event.window_kind {
        WindowKind::FiveHours => {
            let total_minutes = dur.whole_minutes();
            let hours = total_minutes / 60;
            let minutes = total_minutes % 60;
            if hours > 0 && minutes > 0 {
                format!("{}h{}m", hours, minutes)
            } else if hours > 0 {
                format!("{}h", hours)
            } else {
                format!("{}m", minutes)
            }
        }
        WindowKind::SevenDays => {
            let total_hours = dur.whole_hours();
            let days = total_hours / 24;
            let hours = total_hours % 24;
            if days > 0 && hours > 0 {
                format!("{}d{}h", days, hours)
            } else if days > 0 {
                format!("{}d", days)
            } else {
                format!("{}h", hours)
            }
        }
    };

    format!(
        "{} {} remaining: {}",
        display_provider(event.provider),
        display_window(event.window_kind),
        remaining,
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
