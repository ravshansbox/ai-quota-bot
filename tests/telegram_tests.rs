use ai_quota_bot::{
    model::{ProviderKind, ResetEvent, WindowKind},
    telegram::{TelegramClient, format_reset_message},
};
use httpmock::{Method::POST, MockServer};
use reqwest::Client;
use serde_json::json;
use time::macros::datetime;

fn event() -> ResetEvent {
    ResetEvent {
        provider: ProviderKind::Claude,
        plan: "max".into(),
        window_kind: WindowKind::FiveHours,
        reset_at: datetime!(2026-06-29 12:00 UTC),
        previous_window_id: Some("old".into()),
        current_window_id: Some("new".into()),
    }
}

#[test]
fn telegram_message_matches_expected_format() {
    assert_eq!(
        format_reset_message(&event()),
        "Claude 5h quota reset at 12:00 UTC"
    );
}

fn codex_event() -> ResetEvent {
    ResetEvent {
        provider: ProviderKind::Codex,
        plan: "pro".into(),
        window_kind: WindowKind::SevenDays,
        reset_at: datetime!(2026-07-07 00:00 UTC),
        previous_window_id: Some("old-week".into()),
        current_window_id: Some("new-week".into()),
    }
}

#[test]
fn telegram_message_formats_codex_weekly_reset() {
    assert_eq!(
        format_reset_message(&codex_event()),
        "Codex 7d quota reset at 00:00 UTC"
    );
}

#[tokio::test]
async fn telegram_send_reset_posts_expected_payload() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/botbot-token/sendMessage")
            .json_body(json!({
                "chat_id": "1234",
                "text": "Claude 5h quota reset at 12:00 UTC"
            }));
        then.status(200)
            .header("content-type", "application/json")
            .body(r#"{"ok":true,"result":{"message_id":1}}"#);
    });

    let client = TelegramClient::with_api_base(
        Client::new(),
        "bot-token".into(),
        "1234".into(),
        server.url(""),
    );

    client.send_reset(&event()).await.unwrap();

    mock.assert();
}
