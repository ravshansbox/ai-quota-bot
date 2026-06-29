use crate::{
    error::AppResult,
    model::{ProviderCredentials, QuotaSnapshot},
    providers::{ProviderRequestError, QuotaProvider},
};
use time::{Duration, OffsetDateTime};

pub fn should_refresh(creds: &ProviderCredentials, now: OffsetDateTime, leeway: Duration) -> bool {
    match creds.expires_at {
        Some(expires_at) => expires_at <= now + leeway,
        None => false,
    }
}

pub async fn fetch_with_refresh<P: QuotaProvider>(
    provider: &P,
    creds: &ProviderCredentials,
    now: OffsetDateTime,
) -> AppResult<(ProviderCredentials, Vec<QuotaSnapshot>)> {
    let active_creds = if should_refresh(creds, now, Duration::minutes(5)) {
        provider.refresh_credentials(creds).await?
    } else {
        creds.clone()
    };

    match provider.fetch_snapshots(&active_creds).await {
        Ok(snapshots) => Ok((active_creds, snapshots)),
        Err(ProviderRequestError::Authentication) => {
            let refreshed = provider.refresh_credentials(&active_creds).await?;
            let snapshots = provider
                .fetch_snapshots(&refreshed)
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            Ok((refreshed, snapshots))
        }
        Err(ProviderRequestError::Other(err)) => Err(err),
    }
}
