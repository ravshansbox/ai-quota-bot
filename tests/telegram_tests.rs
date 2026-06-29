use ai_quota_bot::{
    model::{ProviderKind, ResetEvent, WindowKind},
    telegram::{TelegramClient, format_reset_message},
};
use httpmock::{Method::POST, MockServer};
use reqwest::Client;
use time::macros::datetime;

fn event() -> ResetEvent {
    ResetEvent {
        provider: ProviderKind::Claude,
        window_kind: WindowKind::FiveHours,
        reset_at: datetime!(2026-06-29 12:00 UTC),
        usage: Some(0),
        limit: Some(100),
    }
}

fn codex_event() -> ResetEvent {
    ResetEvent {
        provider: ProviderKind::Codex,
        window_kind: WindowKind::SevenDays,
        reset_at: datetime!(2026-07-07 00:00 UTC),
        usage: Some(0),
        limit: Some(100),
    }
}

#[test]
fn telegram_message_matches_expected_format() {
    let now = datetime!(2026-06-29 09:00 UTC);
    assert_eq!(
        format_reset_message(&event(), now),
        "📊 Quota summary\nClaude: 5h 0% used (3h)"
    );
}

#[test]
fn telegram_message_formats_codex_weekly_reset() {
    let now = datetime!(2026-07-04 00:00 UTC);
    assert_eq!(
        format_reset_message(&codex_event(), now),
        "📊 Quota summary\nCodex: 7d 0% used (3d)"
    );
}

#[tokio::test]
async fn telegram_send_reset_posts_expected_payload() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/botbot-token/sendMessage")
            .body_contains("1234")
            .body_contains("📊 Quota summary")
            .body_contains("Claude: 5h 0% used");
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
