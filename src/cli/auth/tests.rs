use super::provider::{AuthMethod, default_provider_options, resolve_provider_name};
use rho_harness_core::provider::ProviderId;

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

#[test]
fn default_provider_options_cover_all_variants_and_openrouter_label() {
    let options = default_provider_options();

    // Verify all canonical ProviderId variants are present
    for provider in ProviderId::ALL {
        assert!(
            options.iter().any(|(id, _)| *id == provider.as_str()),
            "missing provider {provider} in default options"
        );
    }

    // Verify OpenRouter option specifies OAuth or API key
    let openrouter_opt = options
        .iter()
        .find(|(id, _)| *id == "openrouter")
        .expect("openrouter must be present");
    assert!(
        openrouter_opt.1.contains("OAuth or API key"),
        "OpenRouter description should mention OAuth or API key: {}",
        openrouter_opt.1
    );
}

#[test]
fn auth_method_equality() {
    assert_eq!(AuthMethod::OAuth, AuthMethod::OAuth);
    assert_eq!(AuthMethod::ApiKey, AuthMethod::ApiKey);
    assert_ne!(AuthMethod::OAuth, AuthMethod::ApiKey);
}
