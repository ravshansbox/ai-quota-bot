use std::collections::HashMap;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    Claude,
    Codex,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowKind {
    FiveHours,
    SevenDays,
}

impl WindowKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FiveHours => "5h",
            Self::SevenDays => "7d",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCredentials {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub account_id: Option<String>,
    pub raw_source: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaSnapshot {
    pub provider: ProviderKind,
    pub plan: String,
    pub window_kind: WindowKind,
    pub window_id: Option<String>,
    pub reset_at: OffsetDateTime,
    pub usage: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetEvent {
    pub provider: ProviderKind,
    pub plan: String,
    pub window_kind: WindowKind,
    pub reset_at: OffsetDateTime,
    pub previous_window_id: Option<String>,
    pub current_window_id: Option<String>,
}
