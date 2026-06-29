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

#[test]
fn unchanged_snapshot_emits_no_event() {
    let mut detector = ResetDetector::default();
    let snapshot = snapshot(datetime!(2026-06-29 12:00 UTC), "a");

    detector.detect(vec![snapshot.clone()]);
    let events = detector.detect(vec![snapshot]);

    assert!(events.is_empty());
}

#[test]
fn window_id_change_alone_emits_event() {
    let mut detector = ResetDetector::default();
    let reset_at = datetime!(2026-06-29 12:00 UTC);

    detector.detect(vec![snapshot(reset_at, "a")]);
    let events = detector.detect(vec![snapshot(reset_at, "b")]);

    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.provider, ProviderKind::Claude);
    assert_eq!(event.plan, "max");
    assert_eq!(event.window_kind, WindowKind::FiveHours);
    assert_eq!(event.reset_at, reset_at);
    assert_eq!(event.previous_window_id.as_deref(), Some("a"));
    assert_eq!(event.current_window_id.as_deref(), Some("b"));
}
