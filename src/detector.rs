use crate::model::{ProviderKind, QuotaSnapshot, ResetEvent, WindowKind};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SnapshotKey {
    provider: ProviderKind,
    window_kind: WindowKind,
}

#[derive(Debug, Clone)]
struct CachedState {
    usage: Option<u64>,
}

/// A reset is only worth reporting when quota is actually freed, so any
/// downward move in usage counts, down to a single point. Percentages are
/// 0..=100.
const USAGE_DROP_THRESHOLD: u64 = 1;

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

            next.insert(key, CachedState { usage: snapshot.usage });
        }

        self.previous = next;
        self.initialized = true;
        events
    }
}

/// A genuine reset frees quota, so we detect it purely from a drop in usage.
/// The `reset_at` boundary is unreliable for this: it can creep forward by a
/// few seconds between polls without any quota being freed, which previously
/// produced spurious "reset detected" notifications even as usage rose.
fn is_reset(prev: &CachedState, current: &QuotaSnapshot) -> bool {
    match (prev.usage, current.usage) {
        (Some(previous_usage), Some(current_usage)) => {
            previous_usage.saturating_sub(current_usage) >= USAGE_DROP_THRESHOLD
        }
        _ => false,
    }
}
