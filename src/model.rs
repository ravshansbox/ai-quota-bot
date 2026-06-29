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
    pub reset_at: OffsetDateTime,
    pub usage: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetEvent {
    pub provider: ProviderKind,
    pub window_kind: WindowKind,
    pub reset_at: OffsetDateTime,
    pub usage: Option<u64>,
    pub limit: Option<u64>,
}

/// Format remaining time as "3h", "1h 28m", "6d 15h", "0m".
pub fn format_remaining(
    window_kind: WindowKind,
    reset_at: OffsetDateTime,
    now: OffsetDateTime,
) -> String {
    let dur = if reset_at > now {
        reset_at - now
    } else {
        return "0m".to_string();
    };

    match window_kind {
        WindowKind::FiveHours => {
            let total_minutes = dur.whole_minutes();
            let hours = total_minutes / 60;
            let minutes = total_minutes % 60;
            if hours > 0 && minutes > 0 {
                format!("{}h {}m", hours, minutes)
            } else if hours > 0 {
                format!("{}h", hours)
            } else {
                format!("{}m", minutes)
            }
        }
        WindowKind::SevenDays => {
            let total_hours = dur.whole_hours();
            let days = total_hours / 24;
            let hours = total_hours % 24;
            if days > 0 && hours > 0 {
                format!("{}d {}h", days, hours)
            } else if days > 0 {
                format!("{}d", days)
            } else {
                format!("{}h", hours)
            }
        }
    }
}
