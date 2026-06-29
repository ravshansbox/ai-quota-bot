pub fn claude_usage_response(reset_at: &str) -> String {
    format!(
        r#"{{"plan":"max","window_kind":"5h","window_id":"claude-window","reset_at":"{}","usage":12,"limit":100}}"#,
        reset_at
    )
}

pub fn codex_usage_response(reset_at: &str) -> String {
    format!(
        r#"{{"plan":"pro","window_kind":"7d","window_id":"codex-window","reset_at":"{}","usage":3,"limit":50}}"#,
        reset_at
    )
}
