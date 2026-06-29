use ai_quota_bot::{auth::load_credentials_map, model::ProviderKind};
use std::path::Path;

#[test]
fn auth_loader_extracts_claude_and_codex_credentials() {
    let creds = load_credentials_map(Path::new("tests/fixtures/auth/sample_auth.json")).unwrap();
    assert_eq!(creds[&ProviderKind::Claude].access_token, "claude-access");
    assert_eq!(
        creds[&ProviderKind::Codex].refresh_token.as_deref(),
        Some("codex-refresh")
    );
}
