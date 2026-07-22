use crate::model::{ProviderKind, QuotaSnapshot, ResetEvent, WindowKind};
use std::collections::HashMap;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SnapshotKey {
    provider: ProviderKind,
    window_kind: WindowKind,
}

#[derive(Debug, Clone)]
struct CachedState {
    usage: Option<u64>,
    reset_at: Option<OffsetDateTime>,
}

/// When a window has no `reset_at` we cannot rely on the boundary rolling
/// forward, so we fall back to a usage drop. Require a large drop so ordinary
/// rolling-window / rounding jitter in `used_percent` does not masquerade as a
/// reset. Percentages are 0..=100.
const USAGE_DROP_THRESHOLD: u64 = 30;

#[derive(Default)]
pub struct ResetDetector {
    previous: HashMap<SnapshotKey, CachedState>,
    initialized: bool,
}

impl ResetDetector {
    pub fn detect(&mut self, current: Vec<QuotaSnapshot>) -> Vec<ResetEvent> {
        let mut events = Vec::new();
        let mut next = HashMap::new();

        for snapshot in current {
            let key = SnapshotKey {
                provider: snapshot.provider,
                window_kind: snapshot.window_kind,
            };

            if self.initialized
                && let Some(prev) = self.previous.get(&key)
                && is_reset(prev, &snapshot)
            {
                events.push(ResetEvent {
                    provider: snapshot.provider,
                    window_kind: snapshot.window_kind,
                    reset_at: snapshot.reset_at,
                    usage: snapshot.usage,
                    limit: snapshot.limit,
                });
            }

            next.insert(
                key,
                CachedState {
                    usage: snapshot.usage,
                    reset_at: snapshot.reset_at,
                },
            );
        }

        self.previous = next;
        self.initialized = true;
        events
    }
}

/// A genuine reset is detected when the window boundary rolls forward to a new
/// window. That timestamp only changes on a real reset, so it is immune to the
/// small downward jitter in `used_percent` that occurs on ordinary polls.
///
/// When `reset_at` is unavailable for the window we fall back to requiring a
/// large drop in usage, which still ignores routine jitter.
fn is_reset(prev: &CachedState, current: &QuotaSnapshot) -> bool {
    match (prev.reset_at, current.reset_at) {
        (Some(previous_reset), Some(current_reset)) => current_reset > previous_reset,
        _ => match (prev.usage, current.usage) {
            (Some(previous_usage), Some(current_usage)) => {
                previous_usage.saturating_sub(current_usage) >= USAGE_DROP_THRESHOLD
            }
            _ => false,
        },
    }
}
