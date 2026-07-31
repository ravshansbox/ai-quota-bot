use crate::{
    auth::load_credentials_map,
    auth_refresh::fetch_with_refresh,
    config::AppConfig,
    detector::ResetDetector,
    error::AppResult,
    model::{ProviderKind, QuotaSnapshot, format_remaining},
    providers::QuotaProvider,
    telegram::{ResetNotifier, format_summary_body, format_summary_message},
};
use std::{
    collections::HashSet,
    process::{Command, Stdio},
    time::Duration,
};
use time::OffsetDateTime;
use tokio::time::sleep;
use tracing::{info, warn};

pub struct Daemon<P1, P2, N> {
    pub config: AppConfig,
    pub notifier: N,
    pub claude: P1,
    pub codex: P2,
    pub detector: ResetDetector,
    /// Latest snapshots from the most recent successful poll, used so reset
    /// notifications can show the full per-provider line (both windows).
    latest_snapshots: Vec<QuotaSnapshot>,
    /// `message_id` of the startup summary message, so subsequent poll
    /// cycles can edit it in place with fresh quota info instead of
    /// posting a new message every time.
    summary_message_id: Option<i64>,
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
            latest_snapshots: Vec::new(),
            summary_message_id: None,
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
            info!(
                provider = event.provider.as_str(),
                window = event.window_kind.as_str(),
                "quota increase detected"
            );
            providers_to_notify.insert(event.provider);
        }

        let mut posted_other_messages = false;
        for provider in providers_to_notify {
            let message = format_summary_message(&self.latest_snapshots, Some(&[provider]), now);
            if !message.is_empty() {
                match self.notifier.notify_text(&message).await {
                    Ok(_) => posted_other_messages = true,
                    Err(e) => {
                        warn!(provider = provider.as_str(), error = %e, "failed to send reset notification")
                    }
                }
            }
            self.ping_provider(provider);
        }

        // Keep the status message live and last in the conversation: edit it in
        // place normally, but when other messages were posted this cycle,
        // delete and repost it so it stays at the bottom.
        if posted_other_messages
            && let Some(message_id) = self.summary_message_id.take()
            && let Err(e) = self.notifier.delete_text(message_id).await
        {
            warn!(error = %e, "failed to delete stale status message");
        }
        self.update_summary_message(now).await;

        snapshots
    }

    pub async fn run_forever(&mut self) -> AppResult<()> {
        // Run the first cycle immediately; it posts the live status message and
        // every later cycle edits it in place.
        let now = OffsetDateTime::now_utc();
        self.run_cycle_at(now).await;

        let interval_secs = self.config.poll_interval_secs;

        loop {
            let now = OffsetDateTime::now_utc();

            // Sleep until the next clock-aligned poll boundary.
            let secs_today =
                now.hour() as u64 * 3600 + now.minute() as u64 * 60 + now.second() as u64;
            let elapsed = secs_today % interval_secs;
            let poll_delay = Duration::from_secs(interval_secs - elapsed);

            tokio::select! {
                _ = sleep(poll_delay) => {
                    let now = OffsetDateTime::now_utc();
                    self.run_cycle_at(now).await;
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("shutdown signal received");
                    return Ok(());
                }
            }
        }
    }

    fn ping_provider(&self, provider: ProviderKind) {
        let spawn_result = Command::new("pi")
            .args([
                "--no-session",
                "--no-context-files",
                "--no-tools",
                "--model",
                provider.cli_model_name(),
                "-p",
                "hi",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match spawn_result {
            Ok(_) => info!(provider = provider.as_str(), "provider ping spawned"),
            Err(error) => warn!(provider = provider.as_str(), error = %error, "provider ping failed to spawn"),
        }
    }

    /// Post the live status message on the first cycle, then edit it in place
    /// on every subsequent cycle. No-op when there are no snapshots to show.
    async fn update_summary_message(&mut self, now: OffsetDateTime) {
        let Some(message) = self.format_status_message(&self.latest_snapshots.clone(), now) else {
            return;
        };
        match self.summary_message_id {
            Some(message_id) => {
                if let Err(e) = self.notifier.edit_text(message_id, &message).await {
                    warn!(error = %e, "failed to update status message");
                }
            }
            None => match self.notifier.notify_text(&message).await {
                Ok(message_id) => self.summary_message_id = message_id,
                Err(e) => warn!(error = %e, "failed to send status message"),
            },
        }
    }

    /// Build the live status message shown as the persistent, edited-in-place
    /// summary. Returns `None` when there are no snapshots to display.
    fn format_status_message(
        &self,
        snapshots: &[QuotaSnapshot],
        now: OffsetDateTime,
    ) -> Option<String> {
        let body = format_summary_body(snapshots, None, now);
        if body.is_empty() {
            return None;
        }
        Some(format!(
            "📊 Quota status\n{}\n\nUpdated {:02}:{:02} UTC",
            body,
            now.hour(),
            now.minute(),
        ))
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
            Ok(provider_snapshots) => snapshots.extend(provider_snapshots),
            Err(error) => {
                warn!(provider = provider.kind().as_str(), error = %error, "provider poll failed")
            }
        }
    }
}
