use crate::model::{ProviderKind, QuotaSnapshot, ResetEvent, WindowKind};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SnapshotKey {
    provider: ProviderKind,
    plan: String,
    window_kind: WindowKind,
}

/// Cached usage value used to detect resets.
#[derive(Debug, Clone)]
struct CachedState {
    used_pct: u64,
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
                plan: snapshot.plan.clone(),
                window_kind: snapshot.window_kind,
            };

            let used_pct = snapshot.usage.unwrap_or(0);

            if self.initialized
                && let Some(prev) = self.previous.get(&key)
            {
                // A real reset means usage dropped significantly (the window
                // refreshed back to a low used %). Small fluctuations within
                // a few percent are just polling noise.
                let dropped = prev.used_pct.saturating_sub(used_pct);
                let timestamp_changed =
                    snapshot.reset_at.unix_timestamp_nanos() != prev.reset_at_ms;
                if dropped >= 5 && timestamp_changed {
                    events.push(ResetEvent {
                        provider: snapshot.provider,
                        plan: snapshot.plan.clone(),
                        window_kind: snapshot.window_kind,
                        reset_at: snapshot.reset_at,
                        previous_window_id: None,
                        current_window_id: None,
                    });
                }
            }

            next.insert(
                key,
                CachedState {
                    used_pct,
                    reset_at_ms: snapshot.reset_at.unix_timestamp_nanos(),
                },
            );
        }

        self.previous = next;
        self.initialized = true;
        events
    }
}
