use ai_quota_bot::config::AppConfig;
use std::{
    env,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    env_lock().lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn capture(vars: &[&'static str]) -> Self {
        let saved = vars
            .iter()
            .map(|var| (*var, env::var(var).ok()))
            .collect();

        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (var, value) in &self.saved {
            unsafe {
                match value {
                    Some(value) => env::set_var(var, value),
                    None => env::remove_var(var),
                }
            }
        }
    }
}

#[test]
fn config_uses_defaults_when_optional_values_missing() {
    let _lock = lock_env();
    let _env = EnvGuard::capture(&[
        "AI_QUOTA_AUTH_PATH",
        "AI_QUOTA_POLL_INTERVAL_SECS",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_CHAT_ID",
        "HOME",
    ]);

    unsafe {
        env::remove_var("AI_QUOTA_AUTH_PATH");
        env::remove_var("AI_QUOTA_POLL_INTERVAL_SECS");
        env::set_var("TELEGRAM_BOT_TOKEN", "bot-token");
        env::set_var("TELEGRAM_CHAT_ID", "1234");
    }

    let config = AppConfig::from_env().unwrap();
    assert_eq!(config.telegram_chat_id, "1234");
    assert_eq!(config.poll_interval_secs, 600);
    assert!(config.auth_path.ends_with(PathBuf::from(".pi/agent/auth.json")));
}

#[test]
fn config_uses_explicit_auth_path_override() {
    let _lock = lock_env();
    let _env = EnvGuard::capture(&[
        "AI_QUOTA_AUTH_PATH",
        "AI_QUOTA_POLL_INTERVAL_SECS",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_CHAT_ID",
        "HOME",
    ]);

    unsafe {
        env::set_var("AI_QUOTA_AUTH_PATH", "/tmp/custom-auth.json");
        env::remove_var("AI_QUOTA_POLL_INTERVAL_SECS");
        env::set_var("TELEGRAM_BOT_TOKEN", "bot-token");
        env::set_var("TELEGRAM_CHAT_ID", "1234");
        env::set_var("HOME", "/tmp/ignored-home");
    }

    let config = AppConfig::from_env().unwrap();

    assert_eq!(config.auth_path, PathBuf::from("/tmp/custom-auth.json"));
}

#[test]
fn config_errors_when_required_telegram_values_missing() {
    let _lock = lock_env();
    let _env = EnvGuard::capture(&[
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_CHAT_ID",
        "AI_QUOTA_AUTH_PATH",
        "HOME",
    ]);

    unsafe {
        env::remove_var("TELEGRAM_BOT_TOKEN");
        env::remove_var("TELEGRAM_CHAT_ID");
        env::set_var("HOME", "/tmp/test-home");
    }

    let error = AppConfig::from_env().unwrap_err().to_string();
    assert!(error.contains("TELEGRAM_BOT_TOKEN"));
}

#[test]
fn config_errors_when_auth_path_and_home_are_unavailable() {
    let _lock = lock_env();
    let _env = EnvGuard::capture(&[
        "AI_QUOTA_AUTH_PATH",
        "AI_QUOTA_POLL_INTERVAL_SECS",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_CHAT_ID",
        "HOME",
    ]);

    unsafe {
        env::remove_var("AI_QUOTA_AUTH_PATH");
        env::remove_var("AI_QUOTA_POLL_INTERVAL_SECS");
        env::remove_var("HOME");
        env::set_var("TELEGRAM_BOT_TOKEN", "bot-token");
        env::set_var("TELEGRAM_CHAT_ID", "1234");
    }

    let error = AppConfig::from_env().unwrap_err().to_string();

    assert!(error.contains("HOME"));
}

#[test]
fn config_errors_when_poll_interval_is_invalid() {
    let _lock = lock_env();
    let _env = EnvGuard::capture(&[
        "AI_QUOTA_AUTH_PATH",
        "AI_QUOTA_POLL_INTERVAL_SECS",
        "TELEGRAM_BOT_TOKEN",
        "TELEGRAM_CHAT_ID",
        "HOME",
    ]);

    unsafe {
        env::set_var("AI_QUOTA_POLL_INTERVAL_SECS", "not-a-number");
        env::set_var("TELEGRAM_BOT_TOKEN", "bot-token");
        env::set_var("TELEGRAM_CHAT_ID", "1234");
        env::set_var("HOME", "/tmp/test-home");
    }

    let error = AppConfig::from_env().unwrap_err().to_string();

    assert!(error.contains("invalid AI_QUOTA_POLL_INTERVAL_SECS"));
}
