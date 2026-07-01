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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaSnapshot {
    pub provider: ProviderKind,
    pub window_kind: WindowKind,
    pub window_id: Option<String>,
    pub reset_at: Option<OffsetDateTime>,
    pub usage: Option<u64>,
    pub limit: Option<u64>,
    /// OpenAI Codex "reset credits" available to instantly reset the
    /// weekly window. Always 0 for Claude.
    pub resets_available: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetEvent {
    pub provider: ProviderKind,
    pub window_kind: WindowKind,
    pub reset_at: Option<OffsetDateTime>,
    pub usage: Option<u64>,
    pub limit: Option<u64>,
}

/// Format remaining time as "3h 2m", "1h 28m", "6d 15h", "42m", "now", or
/// "unknown" when `reset_at` is not known.
pub fn format_remaining(reset_at: Option<OffsetDateTime>, now: OffsetDateTime) -> String {
    let Some(reset_at) = reset_at else {
        return "unknown".to_string();
    };

    let dur = if reset_at > now {
        reset_at - now
    } else {
        return "now".to_string();
    };

    let total_minutes = dur.whole_minutes();
    let days = total_minutes / (24 * 60);
    let hours = (total_minutes / 60) % 24;
    let minutes = total_minutes % 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        "now".to_string()
    }
}
