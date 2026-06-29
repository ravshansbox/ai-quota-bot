use ai_quota_bot::{
    config::AppConfig,
    daemon::Daemon,
    providers::{claude::ClaudeProvider, codex::CodexProvider},
    telegram::TelegramClient,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::from_env()?;
    tracing::info!(
        poll_interval_secs = config.poll_interval_secs,
        auth_path = %config.auth_path.display(),
        "starting ai-quota-bot"
    );

    let notifier = TelegramClient::new(
        config.telegram_bot_token.clone(),
        config.telegram_chat_id.clone(),
    );
    let claude = ClaudeProvider::new(
        std::env::var("AI_QUOTA_CLAUDE_BASE_URL")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
    );
    let codex = CodexProvider::new(
        std::env::var("AI_QUOTA_CODEX_BASE_URL")
            .unwrap_or_else(|_| "https://chatgpt.com".to_string()),
    );

    let mut daemon = Daemon::new(config, notifier, claude, codex);
    daemon.run_forever().await
}
