use crate::{
    error::AppResult,
    model::{ProviderCredentials, ProviderKind},
};
use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::Path};
use time::OffsetDateTime;

/// Matches the real `~/.pi/agent/auth.json` format.
#[derive(Debug, Deserialize, Serialize)]
struct RawAuthFile {
    #[serde(alias = "claude")]
    anthropic: Option<RawProviderAuth>,
    #[serde(alias = "codex")]
    #[serde(rename = "openai-codex")]
    openai_codex: Option<RawProviderAuth>,
}

#[derive(Debug, Deserialize, Serialize)]
struct RawProviderAuth {
    /// The OAuth type discriminator (e.g. "oauth")
    #[serde(rename = "type", default = "default_oauth_type")]
    type_: String,
    #[serde(rename = "access")]
    access_token: String,
    #[serde(rename = "refresh")]
    refresh_token: Option<String>,
    /// epoch milliseconds
    #[serde(rename = "expires")]
    expires_ms: Option<i64>,
    #[serde(rename = "accountId", skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
}

fn default_oauth_type() -> String {
    "oauth".to_string()
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
                .anthropic
                .ok_or_else(|| anyhow!("missing anthropic credentials"))?,
        )?,
    );
    out.insert(
        ProviderKind::Codex,
        convert(
            parsed
                .openai_codex
                .ok_or_else(|| anyhow!("missing openai-codex credentials"))?,
        )?,
    );
    Ok(out)
}

fn convert(raw: RawProviderAuth) -> AppResult<ProviderCredentials> {
    let expires_at = raw
        .expires_ms
        .map(|ms| {
            OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
                .context("invalid expires epoch ms")
        })
        .transpose()?;

    Ok(ProviderCredentials {
        access_token: raw.access_token,
        refresh_token: raw.refresh_token,
        expires_at,
        account_id: raw.account_id,
    })
}

/// Write updated credentials back to the auth.json file.
pub fn persist_credentials(
    path: &Path,
    kind: ProviderKind,
    creds: &ProviderCredentials,
) -> AppResult<()> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut parsed: RawAuthFile = serde_json::from_str(&raw).context("invalid auth json")?;

    let target = match kind {
        ProviderKind::Claude => &mut parsed.anthropic,
        ProviderKind::Codex => &mut parsed.openai_codex,
    };

    let entry = target
        .as_mut()
        .ok_or_else(|| anyhow!("missing {} entry in auth file", kind.as_str()))?;
    entry.access_token = creds.access_token.clone();
    entry.refresh_token.clone_from(&creds.refresh_token);
    entry.expires_ms = creds
        .expires_at
        .map(|dt| dt.unix_timestamp_nanos() as i64 / 1_000_000);
    entry.account_id.clone_from(&creds.account_id);

    let out = serde_json::to_string_pretty(&parsed).context("failed to serialize updated auth")?;
    fs::write(path, &out).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}
