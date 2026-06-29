use ai_quota_bot::{detector::ResetDetector, model::{ProviderKind, QuotaSnapshot, WindowKind}};
use time::macros::datetime;

fn snapshot(reset_at: time::OffsetDateTime, window_id: &str) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Claude,
        plan: "max".into(),
        window_kind: WindowKind::FiveHours,
        window_id: Some(window_id.into()),
        reset_at,
        usage: Some(42),
        limit: Some(100),
    }
}

#[test]
fn first_poll_initializes_without_alert() {
    let mut detector = ResetDetector::default();
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a")]);
    assert!(events.is_empty());
}

#[test]
fn later_reset_timestamp_emits_event() {
    let mut detector = ResetDetector::default();
    detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a")]);
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 17:00 UTC), "b")]);
    assert_eq!(events.len(), 1);
}
