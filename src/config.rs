use crate::error::AppResult;
use anyhow::{Context, anyhow};
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
        let telegram_bot_token =
            env::var("TELEGRAM_BOT_TOKEN").map_err(|_| anyhow!("missing TELEGRAM_BOT_TOKEN"))?;
        let telegram_chat_id =
            env::var("TELEGRAM_CHAT_ID").map_err(|_| anyhow!("missing TELEGRAM_CHAT_ID"))?;

        let auth_path = env::var("AI_QUOTA_AUTH_PATH")
            .map(PathBuf::from)
            .or_else(|_| {
                env::var("HOME")
                    .map(|home| PathBuf::from(home).join(".pi/agent/auth.json"))
                    .map_err(|_| anyhow!("missing AI_QUOTA_AUTH_PATH and HOME"))
            })?;

        let poll_interval_secs = env::var("AI_QUOTA_POLL_INTERVAL_SECS")
            .ok()
            .map(|raw| {
                raw.parse::<u64>()
                    .context("invalid AI_QUOTA_POLL_INTERVAL_SECS")
            })
            .transpose()?
            .unwrap_or(600);
        if poll_interval_secs == 0 {
            return Err(anyhow!(
                "AI_QUOTA_POLL_INTERVAL_SECS must be greater than 0"
            ));
        }

        Ok(Self {
            telegram_bot_token,
            telegram_chat_id,
            auth_path,
            poll_interval_secs,
        })
    }
}
