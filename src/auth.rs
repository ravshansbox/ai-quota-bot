use crate::{
    error::AppResult,
    model::{ProviderCredentials, ProviderKind},
};
use anyhow::{Context, anyhow};
use serde::Deserialize;
use std::{collections::HashMap, fs, path::Path};
use time::OffsetDateTime;

/// Matches the real `~/.pi/agent/auth.json` format.
#[derive(Debug, Deserialize)]
struct RawAuthFile {
    #[serde(alias = "claude")]
    anthropic: Option<RawProviderAuth>,
    #[serde(alias = "codex")]
    #[serde(rename = "openai-codex")]
    openai_codex: Option<RawProviderAuth>,
}

#[derive(Debug, Deserialize)]
struct RawProviderAuth {
    /// ignore the `type` discriminator
    #[serde(rename = "access")]
    access_token: String,
    #[serde(rename = "refresh")]
    refresh_token: Option<String>,
    /// epoch milliseconds
    #[serde(rename = "expires")]
    expires_ms: Option<i64>,
    #[serde(rename = "accountId")]
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
        raw_source: HashMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_real_auth_format() {
        let json = r#"{
            "anthropic": {
                "type": "oauth",
                "access": "sk-ant-access-token",
                "refresh": "sk-ant-refresh-token",
                "expires": 1782740461641
            },
            "openai-codex": {
                "type": "oauth",
                "access": "codex-access-token",
                "refresh": "codex-refresh-token",
                "expires": 1783507780207,
                "accountId": "acct-123"
            }
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        fs::write(&path, json).unwrap();

        let map = load_credentials_map(&path).unwrap();
        assert_eq!(map.len(), 2);

        let claude = map.get(&ProviderKind::Claude).unwrap();
        assert_eq!(claude.access_token, "sk-ant-access-token");
        assert_eq!(
            claude.refresh_token.as_deref(),
            Some("sk-ant-refresh-token")
        );
        assert!(claude.expires_at.is_some());
        assert_eq!(claude.account_id, None);

        let codex = map.get(&ProviderKind::Codex).unwrap();
        assert_eq!(codex.access_token, "codex-access-token");
        assert_eq!(codex.refresh_token.as_deref(), Some("codex-refresh-token"));
        assert!(codex.expires_at.is_some());
        assert_eq!(codex.account_id.as_deref(), Some("acct-123"));
    }

    #[test]
    fn parse_old_test_fixture() {
        // Keep backward compat with the previous snake_case format via serde aliases
        let json = r#"{
            "claude": {
                "access": "claude-token",
                "refresh": "claude-refresh",
                "expires": 1750000000000,
                "accountId": "claude-account"
            },
            "codex": {
                "access": "codex-token",
                "refresh": "codex-refresh",
                "expires": 1750000000000,
                "accountId": "codex-account"
            }
        }"#;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");
        fs::write(&path, json).unwrap();

        let map = load_credentials_map(&path).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get(&ProviderKind::Claude).unwrap().access_token,
            "claude-token"
        );
        assert_eq!(
            map.get(&ProviderKind::Codex).unwrap().access_token,
            "codex-token"
        );
    }
}
