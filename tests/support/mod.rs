pub fn claude_usage_response() -> String {
    r#"{
        "five_hour": { "utilization": 12, "resets_at": "2026-06-29T17:00:00Z" },
        "seven_day": { "utilization": 33, "resets_at": "2026-07-06T05:59:59.952Z" }
    }"#
    .to_string()
}

pub fn codex_usage_response() -> String {
    r#"{
        "rate_limit": {
            "primary_window": {
                "used_percent": 6,
                "reset_at": 1776880800,
                "limit_window_seconds": 18000
            },
            "secondary_window": {
                "used_percent": 25,
                "reset_at": 1777485600,
                "limit_window_seconds": 604800
            }
        }
    }"#
    .to_string()
}
