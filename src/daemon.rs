use crate::{
    auth::load_credentials_map,
    auth_refresh::fetch_with_refresh,
    config::AppConfig,
    detector::ResetDetector,
    error::AppResult,
    model::{ProviderKind, QuotaSnapshot},
    providers::QuotaProvider,
    telegram::ResetNotifier,
};
use time::OffsetDateTime;
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

    pub async fn run_cycle(&mut self) -> AppResult<()> {
        self.run_cycle_at(OffsetDateTime::now_utc()).await
    }

    pub async fn run_cycle_at(&mut self, now: OffsetDateTime) -> AppResult<()> {
        let mut creds = load_credentials_map(&self.config.auth_path)?;
        let mut snapshots = Vec::new();

        self.collect_provider_snapshots(&self.claude, &mut creds, now, &mut snapshots)
            .await;
        self.collect_provider_snapshots(&self.codex, &mut creds, now, &mut snapshots)
            .await;

        for event in self.detector.detect(snapshots) {
            self.notifier.notify_reset(&event).await?;
        }

        Ok(())
    }

    pub async fn run_forever(&mut self) -> AppResult<()> {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(
            self.config.poll_interval_secs,
        ));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(error) = self.run_cycle().await {
                        warn!(error = %error, "poll cycle failed");
                    }
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("shutdown signal received");
                    return Ok(());
                }
            }
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
