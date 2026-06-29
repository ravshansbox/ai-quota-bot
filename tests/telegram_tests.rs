use ai_quota_bot::{
    model::{ProviderKind, ResetEvent, WindowKind},
    telegram::{format_reset_message, TelegramClient},
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
        "Claude Max 5h quota reset at 12:00 UTC"
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
                "text": "Claude Max 5h quota reset at 12:00 UTC"
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
