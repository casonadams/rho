use super::provider::resolve_provider_name;

#[test]
fn built_in_names_canonicalize_and_custom_names_are_kept() {
    assert_eq!(resolve_provider_name(Some("Google"), "anthropic"), "gemini");
    assert_eq!(
        resolve_provider_name(Some("google-antigravity"), "anthropic"),
        "antigravity"
    );
    assert_eq!(resolve_provider_name(None, "GROQ"), "groq");
    assert_eq!(resolve_provider_name(Some("acme"), "anthropic"), "acme");
    assert_eq!(resolve_provider_name(Some("Acme Cloud"), "anthropic"), "acme cloud");
}
