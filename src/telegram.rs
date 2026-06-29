use crate::{
    error::AppResult,
    model::{ProviderKind, ResetEvent, WindowKind},
};
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use time::{UtcOffset, macros::format_description};

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
        self.send_text(&format_reset_message(event)).await
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

pub fn format_reset_message(event: &ResetEvent) -> String {
    let local = UtcOffset::current_local_offset()
        .map(|offset| event.reset_at.to_offset(offset))
        .unwrap_or(event.reset_at);
    let offset_str = UtcOffset::current_local_offset()
        .map(|o| format!("{o}"))
        .unwrap_or_else(|_| "UTC".to_string());

    let time_str = local
        .format(&format_description!("[hour]:[minute]"))
        .unwrap_or_else(|_| "?".to_string());

    format!(
        "{} {} quota reset at {} {}",
        display_provider(event.provider),
        display_window(event.window_kind),
        time_str,
        offset_str,
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
