use crate::{
    auth::load_credentials_map,
    auth_refresh::fetch_with_refresh,
    config::AppConfig,
    detector::ResetDetector,
    error::AppResult,
    model::{ProviderKind, QuotaSnapshot, WindowKind, format_remaining},
    providers::QuotaProvider,
    telegram::{ResetNotifier, format_summary_message},
};
use std::collections::HashSet;
use std::time::Duration;
use time::OffsetDateTime;
use tokio::time::sleep;
use tracing::{info, warn};

#[derive(Debug, Clone)]
struct ScheduledReset {
    provider: ProviderKind,
    window_kind: WindowKind,
    reset_at: OffsetDateTime,
    usage: Option<u64>,
    next_check_at: OffsetDateTime,
    verify_until: OffsetDateTime,
    done: bool,
}

pub struct Daemon<P1, P2, N> {
    pub config: AppConfig,
    pub notifier: N,
    pub claude: P1,
    pub codex: P2,
    pub detector: ResetDetector,
    /// Pending scheduled reset notifications.
    scheduled: Vec<ScheduledReset>,
    /// Windows for which a scheduled notification has already been fired.
    /// The key is `(provider, window_kind)` because the scheduled and
    /// detector notifications both refer to the *same* quota rollover.
    /// A coarser key is sufficient: two genuine resets cannot occur for
    /// the same window between 10-minute polls.
    pub scheduled_fired: HashSet<(ProviderKind, WindowKind)>,
    /// Latest snapshots from the most recent successful poll. Used so
    /// that reset notifications can show the full per-provider line
    /// (both 5h and 7d windows) matching the startup summary format.
    latest_snapshots: Vec<QuotaSnapshot>,
}

impl<P1, P2, N> Daemon<P1, P2, N>
where
    P1: QuotaProvider,
    P2: QuotaProvider,
    N: ResetNotifier,
{
    pub fn new(config: AppConfig, notifier: N, claude: P1, codex: P2) -> Self {
        Self {
            config,
            notifier,
            claude,
            codex,
            detector: ResetDetector::default(),
            scheduled: Vec::new(),
            scheduled_fired: HashSet::new(),
            latest_snapshots: Vec::new(),
        }
    }

    /// Run one poll cycle and return the collected snapshots.
    pub async fn run_cycle_at(&mut self, now: OffsetDateTime) -> Vec<QuotaSnapshot> {
        info!("poll cycle starting");

        let mut creds = match load_credentials_map(&self.config.auth_path) {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "failed to load credentials");
                return Vec::new();
            }
        };
        let mut snapshots = Vec::new();

        self.collect_provider_snapshots(&self.claude, &mut creds, now, &mut snapshots)
            .await;
        self.collect_provider_snapshots(&self.codex, &mut creds, now, &mut snapshots)
            .await;

        info!(collected = snapshots.len(), "poll cycle complete");
        self.latest_snapshots = snapshots.clone();
        for s in &snapshots {
            let remaining = format_remaining(s.reset_at, now);
            info!(
                provider = s.provider.as_str(),
                window = s.window_kind.as_str(),
                usage = s.usage,
                limit = s.limit,
                remaining = %remaining,
                "snapshot",
            );
        }

        let mut providers_to_notify = HashSet::new();
        for event in self.detector.detect(snapshots.clone()) {
            let key = (event.provider, event.window_kind);
            // Skip if a scheduled notification already fired for this window.
            if self.scheduled_fired.remove(&key) {
                info!(
                    provider = event.provider.as_str(),
                    window = event.window_kind.as_str(),
                    "reset already notified via scheduler, skipping detector duplicate"
                );
                continue;
            }
            info!(
                provider = event.provider.as_str(),
                window = event.window_kind.as_str(),
                "reset detected"
            );
            providers_to_notify.insert(event.provider);
        }

        for provider in providers_to_notify {
            let message = format_summary_message(&self.latest_snapshots, Some(&[provider]), now);
            if !message.is_empty()
                && let Err(e) = self.notifier.notify_text(&message).await
            {
                warn!(provider = provider.as_str(), error = %e, "failed to send reset notification");
            }
        }

        snapshots
    }

    pub async fn run_forever(&mut self) -> AppResult<()> {
        // Run the first cycle immediately and send a startup summary.
        let now = OffsetDateTime::now_utc();
        let snapshots = self.run_cycle_at(now).await;
        self.schedule_from_snapshots(&snapshots);
        self.send_startup_summary(&snapshots, now).await;

        let interval_secs = self.config.poll_interval_secs;

        loop {
            let now = OffsetDateTime::now_utc();

            // Fire any scheduled notifications that are already overdue.
            self.fire_due_scheduled(now).await;

            // Compute time to next clock-aligned poll boundary.
            let secs_today =
                now.hour() as u64 * 3600 + now.minute() as u64 * 60 + now.second() as u64;
            let elapsed = secs_today % interval_secs;
            let poll_delay = Duration::from_secs(interval_secs - elapsed);

            // Check if a scheduled notification is due before the next poll.
            let scheduled_delay = self.next_scheduled_delay(now);

            // Sleep until the soonest event.
            let delay = scheduled_delay.unwrap_or(poll_delay).min(poll_delay);

            tokio::select! {
                _ = sleep(delay) => {
                    let now = OffsetDateTime::now_utc();
                    self.fire_due_scheduled(now).await;

                    // Poll if the full poll timer expired (not an early scheduled wake).
                    if delay == poll_delay {
                        let snapshots = self.run_cycle_at(now).await;
                        self.schedule_from_snapshots(&snapshots);
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("shutdown signal received");
                    return Ok(());
                }
            }
        }
    }

    /// Replace pending scheduled entries with fresh ones from the latest snapshots.
    fn schedule_from_snapshots(&mut self, snapshots: &[QuotaSnapshot]) {
        let now = OffsetDateTime::now_utc();
        // Clear out done entries so we don't accumulate stale ones.
        self.scheduled.retain(|s| !s.done);
        for s in snapshots {
            let Some(reset_at) = s.reset_at else {
                // Unknown reset time: rely on the detector's usage-drop signal only.
                continue;
            };
            if reset_at <= now {
                continue;
            }
            // Replace any existing pending entry for this (provider, window_kind).
            self.scheduled
                .retain(|p| !(p.provider == s.provider && p.window_kind == s.window_kind));
            self.scheduled.push(ScheduledReset {
                provider: s.provider,
                window_kind: s.window_kind,
                reset_at,
                usage: s.usage,
                next_check_at: reset_at,
                verify_until: reset_at + time::Duration::minutes(3),
                done: false,
            });
        }
    }

    /// Return the shortest `Duration` until a pending scheduled notification fires.
    /// Returns `None` when no pending notifications exist.
    fn next_scheduled_delay(&self, now: OffsetDateTime) -> Option<Duration> {
        self.scheduled
            .iter()
            .filter(|s| !s.done)
            .map(|s| {
                let dur = s.next_check_at - now;
                if dur.is_negative() {
                    Duration::ZERO
                } else {
                    Duration::from_secs(dur.whole_seconds().max(0) as u64)
                }
            })
            .min()
    }

    /// Fire all scheduled notifications whose `reset_at` is at or before `now`.
    async fn fire_due_scheduled(&mut self, now: OffsetDateTime) {
        let due: Vec<_> = self
            .scheduled
            .iter()
            .filter(|s| !s.done && s.next_check_at <= now)
            .map(|s| {
                (
                    s.provider,
                    s.window_kind,
                    s.reset_at,
                    s.usage,
                    s.verify_until,
                )
            })
            .collect();

        if due.is_empty() {
            return;
        }

        for (provider, window_kind, _, _, _) in &due {
            self.scheduled_fired.insert((*provider, *window_kind));
        }

        self.run_cycle_at(now).await;

        let mut providers_to_notify = HashSet::new();
        let mut confirmed_keys = HashSet::new();
        let mut stale_keys = HashSet::new();

        for (provider, window_kind, reset_at, usage, verify_until) in due {
            let key = (provider, window_kind);
            let confirmed = self.latest_snapshots.iter().any(|snapshot| {
                snapshot.provider == provider
                    && snapshot.window_kind == window_kind
                    && (snapshot.reset_at.is_some_and(|r| r > reset_at)
                        || matches!((usage, snapshot.usage), (Some(previous), Some(current)) if current < previous))
            });

            if confirmed {
                self.scheduled_fired.remove(&key);
                info!(
                    provider = provider.as_str(),
                    window = window_kind.as_str(),
                    "scheduled reset confirmed"
                );
                providers_to_notify.insert(provider);
                confirmed_keys.insert(key);
            } else {
                self.scheduled_fired.remove(&key);
                if now >= verify_until {
                    warn!(
                        provider = provider.as_str(),
                        window = window_kind.as_str(),
                        "scheduled reset verification expired without fresh provider data"
                    );
                    confirmed_keys.insert(key);
                } else {
                    stale_keys.insert(key);
                }
            }
        }

        for s in &mut self.scheduled {
            let key = (s.provider, s.window_kind);
            if confirmed_keys.contains(&key) {
                s.done = true;
            } else if stale_keys.contains(&key) {
                s.next_check_at = now + time::Duration::seconds(30);
            }
        }

        for provider in providers_to_notify {
            let message = format_summary_message(&self.latest_snapshots, Some(&[provider]), now);
            if !message.is_empty()
                && let Err(e) = self.notifier.notify_text(&message).await
            {
                warn!(provider = provider.as_str(), error = %e, "failed to send scheduled reset notification");
            }
        }
    }

    async fn send_startup_summary(&self, snapshots: &[QuotaSnapshot], now: OffsetDateTime) {
        let summary = format_summary_message(snapshots, None, now);
        if summary.is_empty() {
            info!("no snapshots to summarize on startup");
            return;
        }
        info!("sending startup summary");
        if let Err(e) = self.notifier.notify_text(&summary).await {
            warn!(error = %e, "failed to send startup summary");
        }
    }

    async fn collect_provider_snapshots<P>(
        &self,
        provider: &P,
        creds: &mut std::collections::HashMap<ProviderKind, crate::model::ProviderCredentials>,
        now: OffsetDateTime,
        snapshots: &mut Vec<QuotaSnapshot>,
    ) where
        P: QuotaProvider,
    {
        let Some(provider_creds) = creds.remove(&provider.kind()) else {
            warn!(
                provider = provider.kind().as_str(),
                "provider credentials missing"
            );
            return;
        };

        match fetch_with_refresh(provider, &provider_creds, now).await {
            Ok((_creds, provider_snapshots)) => snapshots.extend(provider_snapshots),
            Err(error) => {
                warn!(provider = provider.kind().as_str(), error = %error, "provider poll failed")
            }
        }
    }
}
