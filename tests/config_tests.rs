use ai_quota_bot::model::{ProviderKind, WindowKind};

#[test]
fn model_enums_are_exposed() {
    assert_eq!(ProviderKind::Claude.as_str(), "claude");
    assert_eq!(WindowKind::FiveHours.as_str(), "5h");
}
