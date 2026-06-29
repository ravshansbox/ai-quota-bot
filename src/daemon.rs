use crate::{
    auth::load_credentials_map,
    auth_refresh::fetch_with_refresh,
    config::AppConfig,
    detector::ResetDetector,
    error::AppResult,
    model::{ProviderKind, QuotaSnapshot, ResetEvent, WindowKind, format_remaining},
    providers::QuotaProvider,
    telegram::ResetNotifier,
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
    limit: Option<u64>,
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
    /// The detector uses this to avoid sending a duplicate on the next poll.
    pub scheduled_fired: HashSet<(ProviderKind, WindowKind)>,
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
        for s in &snapshots {
            let remaining = format_remaining(s.window_kind, s.reset_at, now);
            info!(
                provider = s.provider.as_str(),
                window = s.window_kind.as_str(),
                usage = s.usage,
                limit = s.limit,
                remaining = %remaining,
                "snapshot",
            );
        }

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
            if let Err(e) = self.notifier.notify_reset(&event).await {
                warn!(error = %e, "failed to send reset notification");
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
            if s.reset_at <= now {
                continue;
            }
            // Replace any existing pending entry for this (provider, window_kind).
            self.scheduled
                .retain(|p| !(p.provider == s.provider && p.window_kind == s.window_kind));
            self.scheduled.push(ScheduledReset {
                provider: s.provider,
                window_kind: s.window_kind,
                reset_at: s.reset_at,
                usage: s.usage,
                limit: s.limit,
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
            .filter_map(|s| {
                let dur = s.reset_at - now;
                if dur.is_negative() {
                    Some(Duration::ZERO)
                } else {
                    Some(Duration::from_secs(dur.whole_seconds().max(0) as u64))
                }
            })
            .min()
    }

    /// Fire all scheduled notifications whose `reset_at` is at or before `now`.
    async fn fire_due_scheduled(&mut self, now: OffsetDateTime) {
        for s in &mut self.scheduled {
            if s.done {
                continue;
            }
            if s.reset_at > now {
                continue;
            }
            s.done = true;
            self.scheduled_fired.insert((s.provider, s.window_kind));
            let event = ResetEvent {
                provider: s.provider,
                window_kind: s.window_kind,
                reset_at: s.reset_at,
                usage: s.usage,
                limit: s.limit,
            };
            info!(
                provider = event.provider.as_str(),
                window = event.window_kind.as_str(),
                "scheduled reset notification"
            );
            if let Err(e) = self.notifier.notify_reset(&event).await {
                warn!(error = %e, "failed to send scheduled reset notification");
            }
        }
    }

    async fn send_startup_summary(&self, snapshots: &[QuotaSnapshot], now: OffsetDateTime) {
        if snapshots.is_empty() {
            info!("no snapshots to summarize on startup");
            return;
        }

        // Group snapshots by provider so we can emit one line per provider.
        let mut claude_windows: Vec<&QuotaSnapshot> = Vec::new();
        let mut codex_windows: Vec<&QuotaSnapshot> = Vec::new();

        for s in snapshots {
            match s.provider {
                ProviderKind::Claude => claude_windows.push(s),
                ProviderKind::Codex => codex_windows.push(s),
            }
        }

        let mut lines: Vec<String> = Vec::new();
        for (provider_name, windows) in [("Claude", &claude_windows), ("Codex", &codex_windows)] {
            if windows.is_empty() {
                continue;
            }

            let mut parts: Vec<String> = Vec::new();

            // Ensure 5h comes before 7d for consistent ordering.
            for window_kind in [WindowKind::FiveHours, WindowKind::SevenDays] {
                if let Some(s) = windows.iter().find(|w| w.window_kind == window_kind) {
                    let label = match s.window_kind {
                        WindowKind::FiveHours => "5h",
                        WindowKind::SevenDays => "7d",
                    };
                    let pct = match (s.usage, s.limit) {
                        (Some(u), Some(l)) if l > 0 => format!("{}% used", u * 100 / l),
                        _ => "?".to_string(),
                    };
                    let remaining = format_remaining(s.window_kind, s.reset_at, now);
                    parts.push(format!("{} {} ({})", label, pct, remaining));
                }
            }

            lines.push(format!("{}: {}", provider_name, parts.join(", ")));
        }

        let summary = format!("📊 Quota summary\n{}", lines.join("\n"));
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
