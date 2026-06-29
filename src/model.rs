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
