use crate::model::{ProviderKind, QuotaSnapshot, ResetEvent, WindowKind};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SnapshotKey {
    provider: ProviderKind,
    window_kind: WindowKind,
}

/// Last seen window boundary, used to detect that the window rolled over.
#[derive(Debug, Clone)]
struct CachedState {
    reset_at_ms: i128,
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

            let reset_at_ms = snapshot.reset_at.unix_timestamp_nanos();

            if self.initialized
                && let Some(prev) = self.previous.get(&key)
            {
                // The authoritative reset signal is the window boundary moving
                // forward: the provider handed us a brand new window with a
                // later `reset_at`. Usage dropping is just a side effect of
                // that, so we key on the boundary itself. A strictly-greater
                // comparison ignores the provider nudging the timestamp
                // backwards within the same window.
                if reset_at_ms > prev.reset_at_ms {
                    events.push(ResetEvent {
                        provider: snapshot.provider,
                        window_kind: snapshot.window_kind,
                        reset_at: snapshot.reset_at,
                        usage: snapshot.usage,
                        limit: snapshot.limit,
                    });
                }
            }

            next.insert(key, CachedState { reset_at_ms });
        }

        self.previous = next;
        self.initialized = true;
        events
    }
}
