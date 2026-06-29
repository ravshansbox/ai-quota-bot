use crate::model::{ProviderKind, QuotaSnapshot, ResetEvent, WindowKind};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SnapshotKey {
    provider: ProviderKind,
    plan: String,
    window_kind: WindowKind,
}

#[derive(Default)]
pub struct ResetDetector {
    previous: HashMap<SnapshotKey, QuotaSnapshot>,
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

            if self.initialized {
                if let Some(previous) = self.previous.get(&key) {
                    let reset_advanced = snapshot.reset_at > previous.reset_at;
                    let window_changed = snapshot.window_id != previous.window_id;
                    if reset_advanced || window_changed {
                        events.push(ResetEvent {
                            provider: snapshot.provider,
                            plan: snapshot.plan.clone(),
                            window_kind: snapshot.window_kind,
                            reset_at: snapshot.reset_at,
                            previous_window_id: previous.window_id.clone(),
                            current_window_id: snapshot.window_id.clone(),
                        });
                    }
                }
            }

            next.insert(key, snapshot);
        }

        self.previous = next;
        self.initialized = true;
        events
    }
}
