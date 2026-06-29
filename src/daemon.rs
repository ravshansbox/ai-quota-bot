use crate::{
    auth::load_credentials_map,
    auth_refresh::fetch_with_refresh,
    config::AppConfig,
    detector::ResetDetector,
    error::AppResult,
    model::{ProviderKind, QuotaSnapshot, WindowKind},
    providers::QuotaProvider,
    telegram::ResetNotifier,
};
use std::time::Duration;
use time::{OffsetDateTime, UtcOffset, macros::format_description};
use tokio::time::sleep;
use tracing::{info, warn};

pub struct Daemon<P1, P2, N> {
    pub config: AppConfig,
    pub notifier: N,
    pub claude: P1,
    pub codex: P2,
    pub detector: ResetDetector,
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
        }
    }

    /// Run one poll cycle and return the collected snapshots.
    pub async fn run_cycle_at(&mut self, now: OffsetDateTime) -> Vec<QuotaSnapshot> {
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

        for event in self.detector.detect(snapshots.clone()) {
            if let Err(e) = self.notifier.notify_reset(&event).await {
                warn!(error = %e, "failed to send reset notification");
            }
        }

        snapshots
    }

    pub async fn run_forever(&mut self) -> AppResult<()> {
        // Run the first cycle immediately and send a startup summary.
        let snapshots = self.run_cycle_at(OffsetDateTime::now_utc()).await;
        self.send_startup_summary(&snapshots).await;

        let interval_secs = self.config.poll_interval_secs;

        loop {
            // Sleep until the next clock-aligned boundary so polls
            // land on consistent times (e.g. every 10 min at :00/:10/:20).
            let now = OffsetDateTime::now_utc();
            let secs_today =
                now.hour() as u64 * 3600 + now.minute() as u64 * 60 + now.second() as u64;
            let elapsed = secs_today % interval_secs;
            let delay = Duration::from_secs(interval_secs - elapsed);

            tokio::select! {
                _ = sleep(delay) => {
                    self.run_cycle_at(OffsetDateTime::now_utc()).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("shutdown signal received");
                    return Ok(());
                }
            }
        }
    }

    async fn send_startup_summary(&self, snapshots: &[QuotaSnapshot]) {
        if snapshots.is_empty() {
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
                    let reset = format_reset_time(s.window_kind, s.reset_at);
                    parts.push(format!("{} {} ({})", label, pct, reset));
                }
            }

            lines.push(format!("{}: {}", provider_name, parts.join(", ")));
        }

        let summary = format!("📊 Quota summary\n{}", lines.join("\n"));
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

fn format_reset_time(window_kind: WindowKind, reset_at: OffsetDateTime) -> String {
    let local = UtcOffset::current_local_offset()
        .map(|offset| reset_at.to_offset(offset))
        .unwrap_or(reset_at);
    let offset_str = UtcOffset::current_local_offset()
        .map(|o| format!("{o}"))
        .unwrap_or_else(|_| "UTC".to_string());

    match window_kind {
        // 5h windows reset within hours — show the local time with offset.
        WindowKind::FiveHours => local
            .format(&format_description!("[hour]:[minute]"))
            .map(|t| format!("{} {}", t, offset_str))
            .unwrap_or_else(|_| "?".to_string()),
        // 7d windows reset days out — show the local short date.
        WindowKind::SevenDays => local
            .format(&format_description!("[month repr:short] [day]"))
            .unwrap_or_else(|_| "?".to_string()),
    }
}
