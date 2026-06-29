use crate::{
    error::AppResult,
    model::{ProviderCredentials, ProviderKind, QuotaSnapshot},
};
use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum ProviderRequestError {
    #[error("authentication failed")]
    Authentication,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[async_trait]
pub trait QuotaProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    async fn fetch_snapshots(
        &self,
        creds: &ProviderCredentials,
    ) -> Result<Vec<QuotaSnapshot>, ProviderRequestError>;

    async fn refresh_credentials(&self, creds: &ProviderCredentials) -> AppResult<ProviderCredentials>;
}
