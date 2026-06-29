use ai_quota_bot::config::AppConfig;
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

#[test]
fn config_uses_defaults_when_optional_values_missing() {
    let _guard = env_lock().lock().unwrap();

    unsafe {
        std::env::remove_var("AI_QUOTA_AUTH_PATH");
        std::env::remove_var("AI_QUOTA_POLL_INTERVAL_SECS");
        std::env::set_var("TELEGRAM_BOT_TOKEN", "bot-token");
        std::env::set_var("TELEGRAM_CHAT_ID", "1234");
    }

    let config = AppConfig::from_env().unwrap();
    assert_eq!(config.telegram_chat_id, "1234");
    assert_eq!(config.poll_interval_secs, 600);
    assert!(config.auth_path.ends_with(PathBuf::from(".pi/agent/auth.json")));
}

#[test]
fn config_errors_when_required_telegram_values_missing() {
    let _guard = env_lock().lock().unwrap();

    unsafe {
        std::env::remove_var("TELEGRAM_BOT_TOKEN");
        std::env::remove_var("TELEGRAM_CHAT_ID");
    }

    let error = AppConfig::from_env().unwrap_err().to_string();
    assert!(error.contains("TELEGRAM_BOT_TOKEN"));
}
