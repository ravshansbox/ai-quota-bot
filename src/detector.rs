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
                && let (Some(previous_usage), Some(current_usage)) = (prev.usage, snapshot.usage)
                && current_usage < previous_usage
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
                },
            );
        }

        self.previous = next;
        self.initialized = true;
        events
    }
}
