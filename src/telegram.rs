use crate::{
    error::AppResult,
    model::{ProviderKind, ResetEvent, WindowKind},
};
use reqwest::Client;
use serde::Serialize;
use time::macros::format_description;

#[derive(Clone, Debug)]
pub struct TelegramClient {
    client: Client,
    bot_token: String,
    chat_id: String,
    api_base: String,
}

impl TelegramClient {
    pub fn new(bot_token: String, chat_id: String) -> Self {
        Self::with_api_base(Client::new(), bot_token, chat_id, "https://api.telegram.org")
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
        let url = format!("{}/bot{}/sendMessage", self.api_base, self.bot_token);
        let body = SendMessageBody {
            chat_id: self.chat_id.clone(),
            text: format_reset_message(event),
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
    format!(
        "{} {} {} quota reset at {} UTC",
        display_provider(event.provider),
        capitalize(&event.plan),
        display_window(event.window_kind),
        event
            .reset_at
            .format(&format_description!("[hour]:[minute]"))
            .expect("fixed time format should be valid"),
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
        Some(first) => {
            let mut capitalized = first.to_uppercase().collect::<String>();
            capitalized.push_str(chars.as_str());
            capitalized
        }
        None => String::new(),
    }
}
