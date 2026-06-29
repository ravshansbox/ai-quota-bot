use crate::{
    error::AppResult,
    model::{ProviderCredentials, ProviderKind},
};
use anyhow::{Context, anyhow};
use serde::Deserialize;
use std::{collections::HashMap, fs, path::Path};
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
struct RawAuthFile {
    claude: Option<RawProviderAuth>,
    codex: Option<RawProviderAuth>,
}

#[derive(Debug, Deserialize)]
struct RawProviderAuth {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<String>,
    account_id: Option<String>,
}

pub fn load_credentials_map(path: &Path) -> AppResult<HashMap<ProviderKind, ProviderCredentials>> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed: RawAuthFile = serde_json::from_str(&raw).context("invalid auth json")?;

    let mut out = HashMap::new();
    out.insert(
        ProviderKind::Claude,
        convert(
            parsed
                .claude
                .ok_or_else(|| anyhow!("missing claude credentials"))?,
        )?,
    );
    out.insert(
        ProviderKind::Codex,
        convert(
            parsed
                .codex
                .ok_or_else(|| anyhow!("missing codex credentials"))?,
        )?,
    );
    Ok(out)
}

fn convert(raw: RawProviderAuth) -> AppResult<ProviderCredentials> {
    let expires_at = raw
        .expires_at
        .map(|value| OffsetDateTime::parse(&value, &time::format_description::well_known::Rfc3339))
        .transpose()
        .context("invalid expires_at")?;

    Ok(ProviderCredentials {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token,
        expires_at,
        account_id: raw.account_id,
        raw_source: HashMap::new(),
    })
}
