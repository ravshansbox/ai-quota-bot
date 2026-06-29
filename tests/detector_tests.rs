use ai_quota_bot::{
    detector::ResetDetector,
    model::{ProviderKind, QuotaSnapshot, WindowKind},
};
use time::macros::datetime;

fn snapshot(reset_at: time::OffsetDateTime, window_id: &str, usage: u64) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Claude,
        plan: "max".into(),
        window_kind: WindowKind::FiveHours,
        window_id: Some(window_id.into()),
        reset_at,
        usage: Some(usage),
        limit: Some(100),
    }
}

#[test]
fn first_poll_initializes_without_alert() {
    let mut detector = ResetDetector::default();
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a", 42)]);
    assert!(events.is_empty());
}

#[test]
fn usage_drop_detects_reset() {
    let mut detector = ResetDetector::default();
    // First poll: 42% used
    detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a", 42)]);
    // Second poll: dropped to 0% — reset happened
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 17:00 UTC), "b", 0)]);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].provider, ProviderKind::Claude);
    assert_eq!(events[0].plan, "max");
    assert_eq!(events[0].window_kind, WindowKind::FiveHours);
}

#[test]
fn unchanged_snapshot_emits_no_event() {
    let mut detector = ResetDetector::default();
    let s = snapshot(datetime!(2026-06-29 12:00 UTC), "a", 42);

    detector.detect(vec![s.clone()]);
    let events = detector.detect(vec![s]);

    assert!(events.is_empty());
}

#[test]
fn small_usage_fluctuation_does_not_trigger() {
    let mut detector = ResetDetector::default();
    // First poll: 42% used
    detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a", 42)]);
    // Second poll: 40% used (only 2% drop — noise, not a reset)
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 12:10 UTC), "a", 40)]);
    assert!(events.is_empty());
}

#[test]
fn usage_drop_without_timestamp_change_does_not_trigger() {
    let mut detector = ResetDetector::default();
    let reset_at = datetime!(2026-06-29 12:00 UTC);

    detector.detect(vec![snapshot(reset_at, "a", 42)]);
    // usage dropped but same reset_at timestamp — no real reset
    let events = detector.detect(vec![snapshot(reset_at, "b", 0)]);
    assert!(events.is_empty());
}
