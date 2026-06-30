use ai_quota_bot::{
    detector::ResetDetector,
    model::{ProviderKind, QuotaSnapshot, WindowKind},
};
use time::macros::datetime;

fn snapshot(reset_at: time::OffsetDateTime, window_id: &str, usage: u64) -> QuotaSnapshot {
    QuotaSnapshot {
        provider: ProviderKind::Claude,
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
fn window_boundary_advancing_detects_reset() {
    let mut detector = ResetDetector::default();
    // First poll: window resets at 12:00.
    detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a", 42)]);
    // Second poll: new window with a later reset_at — reset happened.
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 17:00 UTC), "b", 0)]);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].provider, ProviderKind::Claude);
    assert_eq!(events[0].window_kind, WindowKind::FiveHours);
}

#[test]
fn reset_from_low_usage_still_detected() {
    let mut detector = ResetDetector::default();
    // Barely used the window (3%) then it rolled over to 0% — a <5% drop,
    // which the old usage-threshold logic missed. The boundary moved, so it
    // is a real reset.
    detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "a", 3)]);
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 17:00 UTC), "b", 0)]);
    assert_eq!(events.len(), 1);
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
fn usage_dropping_within_same_window_does_not_trigger() {
    let mut detector = ResetDetector::default();
    let reset_at = datetime!(2026-06-29 12:00 UTC);

    // Same window boundary; provider corrected usage downward. Not a reset.
    detector.detect(vec![snapshot(reset_at, "a", 42)]);
    let events = detector.detect(vec![snapshot(reset_at, "b", 0)]);
    assert!(events.is_empty());
}

#[test]
fn reset_at_moving_backwards_does_not_trigger() {
    let mut detector = ResetDetector::default();
    // A spurious earlier reset_at (clock skew / provider jitter) is not a reset.
    detector.detect(vec![snapshot(datetime!(2026-06-29 17:00 UTC), "a", 42)]);
    let events = detector.detect(vec![snapshot(datetime!(2026-06-29 12:00 UTC), "b", 0)]);
    assert!(events.is_empty());
}
