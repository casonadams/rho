use super::provider::{AuthMethod, login_choices, resolve_provider_name, select_login_provider};
use crate::config::Config;

#[test]
fn built_in_names_canonicalize_and_custom_names_are_kept() {
    let cases = [
        (Some("Google"), "anthropic", "gemini"),
        (Some("google-antigravity"), "anthropic", "antigravity"),
        (None, "GROQ", "groq"),
        (Some("acme"), "anthropic", "acme"),
        (Some("Acme Cloud"), "anthropic", "acme cloud"),
    ];
    for (input, default, expected) in cases {
        assert_eq!(resolve_provider_name(input, default), expected);
    }
}

#[test]
fn login_choices_offer_every_builtin_login_then_custom_providers() {
    use rho_harness_core::provider::ProviderId;

    let mut config = Config::default();
    config.providers.insert(
        "custom-llm".to_string(),
        rho_harness_core::config::ProviderConfig {
            base_url: "http://localhost:8000".to_string(),
            key_env: None,
            default_model: None,
        },
    );
    let temp_auth = rho_test_support::TestFile::new();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();
    auth_store.set_key("ollama-cloud", "test-key-not-real").unwrap();

    let choices = login_choices(&config, &auth_store);
    let names: Vec<&str> = choices.iter().map(|c| c.name.as_str()).collect();
    let mut expected: Vec<&str> = ProviderId::ALL
        .into_iter()
        .filter(|&id| id != ProviderId::Local)
        .map(ProviderId::as_str)
        .collect();
    expected.push("custom-llm");
    expected.push("exa-search");
    assert_eq!(names, expected);

    let choice = |name: &str| choices.iter().find(|c| c.name == name).unwrap();
    assert!(choice("ollama-cloud").signed_in);
    assert!(choice("ollama-cloud").description.ends_with("(API key)"));
    assert!(choice("openrouter").description.ends_with("(OAuth or API key)"));
    assert!(!choice("custom-llm").signed_in);
    assert_eq!(choice("exa-search").description, "Exa Web Search (API key)");

    auth_store.set_key("exa-search", "exa-test-key").unwrap();
    let updated = login_choices(&config, &auth_store);
    assert!(updated.iter().find(|c| c.name == "exa-search").unwrap().signed_in);
}

#[test]
fn select_login_provider_lists_each_choice_and_returns_the_pick() {
    use super::provider::LoginChoice;

    let choices = [
        LoginChoice {
            name: "claude".to_string(),
            description: "Claude subscription".to_string(),
            signed_in: true,
        },
        LoginChoice {
            name: "ollama-cloud".to_string(),
            description: "Hosted open models".to_string(),
            signed_in: false,
        },
    ];
    let picked = select_login_provider(&choices, |prompt, items| {
        assert_eq!(prompt, "Select provider to log in:");
        assert_eq!(
            items,
            [
                "claude         Claude subscription  ✓",
                "ollama-cloud   Hosted open models"
            ]
        );
        Ok(1)
    })
    .unwrap();
    assert_eq!(picked, "ollama-cloud");
}

#[test]
fn auth_method_equality() {
    assert_eq!(AuthMethod::OAuth, AuthMethod::OAuth);
    assert_eq!(AuthMethod::ApiKey, AuthMethod::ApiKey);
    assert_ne!(AuthMethod::OAuth, AuthMethod::ApiKey);
}

#[test]
fn read_key_from_reader_reads_clean_lines_and_crlf() {
    let unix_input = b"sk-my-api-key-123\n";
    let key = super::read_key_from_reader(&unix_input[..]).unwrap();
    assert_eq!(key, "sk-my-api-key-123");

    let win_input = b"sk-my-api-key-456\r\n";
    let key = super::read_key_from_reader(&win_input[..]).unwrap();
    assert_eq!(key, "sk-my-api-key-456");
}

#[test]
fn read_key_from_reader_empty_or_whitespace_fails() {
    let empty_input = b"";
    let err = super::read_key_from_reader(&empty_input[..]).unwrap_err();
    assert!(err.to_string().contains("No API key provided on stdin"));

    let whitespace_input = b"   \r\n";
    let err = super::read_key_from_reader(&whitespace_input[..]).unwrap_err();
    assert!(err.to_string().contains("No API key provided on stdin"));
}

#[tokio::test]
async fn login_provider_key_stdin_without_provider_fails() {
    let config = Config::default();
    let temp_auth = rho_test_support::TestFile::new();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();
    let err = super::login_provider(None, true, &config, &mut auth_store)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("Provider name required when using --key-stdin")
    );
}

#[test]
fn default_oauth_provider_checks() {
    use rho_harness_core::provider::ProviderId;

    let cases = [
        (ProviderId::ChatGpt, true),
        (ProviderId::Antigravity, true),
        (ProviderId::ClaudeCode, true),
        (ProviderId::OpenAi, false),
        (ProviderId::OpenRouter, false),
        (ProviderId::Groq, false),
        (ProviderId::Local, false),
    ];
    for (id, expected) in cases {
        assert_eq!(super::is_default_oauth_provider(id), expected);
        if id != ProviderId::OpenRouter {
            assert_eq!(super::should_default_oauth(id).unwrap(), expected);
        }
    }
}

#[test]
fn test_store_api_key_valid_and_empty() {
    let temp_auth = rho_test_support::TestFile::new();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();

    let err = super::store_api_key("gemini", "  ", &mut auth_store).unwrap_err();
    assert!(err.to_string().contains("API key cannot be empty"));

    super::store_api_key("gemini", "test-key-123", &mut auth_store).unwrap();
    assert_eq!(
        auth_store.get_key_sync("gemini").unwrap().as_deref(),
        Some("test-key-123")
    );
}

#[test]
fn test_prompt_logout_selection_maps_invalid_input_to_cancelled() {
    use std::io::Cursor;
    let configured = vec!["anthropic".to_string(), "gemini".to_string()];
    for (input, expected) in [
        ("1\n", Ok("anthropic")),
        ("2\n", Ok("gemini")),
        ("0\n", Err(())),
        ("3\n", Err(())),
        ("abc\n", Err(())),
    ] {
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();
        let result = super::prompt_logout_selection(&mut reader, &mut writer, &configured);
        match expected {
            Ok(name) => assert_eq!(result.unwrap(), name, "{input:?}"),
            Err(()) => assert!(
                matches!(result, Err(crate::error::AppError::Cancelled(ref msg)) if msg == "Logout cancelled"),
                "{input:?}"
            ),
        }
    }
}

#[test]
fn test_prompt_logout_selection() {
    use std::io::Cursor;

    let configured = vec!["anthropic".to_string(), "gemini".to_string()];
    let mut reader = Cursor::new(b"2\n");
    let mut writer = Vec::new();
    let chosen = super::prompt_logout_selection(&mut reader, &mut writer, &configured).unwrap();
    assert_eq!(chosen, "gemini");
    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Select provider credentials to remove:"));
    assert!(output.contains("1. anthropic"));
    assert!(output.contains("2. gemini"));
}

#[test]
fn test_logout_provider_named_and_empty_list() {
    let config = Config::default();
    let temp_auth = rho_test_support::TestFile::new();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();

    assert!(super::logout_provider(None, &config, &mut auth_store).is_ok());

    auth_store.set_key("openrouter", "key-val").unwrap();
    assert!(auth_store.get_key_sync("openrouter").unwrap().is_some());
    assert!(super::logout_provider(Some("openrouter"), &config, &mut auth_store).is_ok());
    assert!(auth_store.get_key_sync("openrouter").unwrap().is_none());

    let err = super::logout_provider(Some("openrouter"), &config, &mut auth_store).unwrap_err();
    assert!(
        err.to_string().contains("No stored credentials for openrouter"),
        "{err}"
    );
}

#[tokio::test]
async fn test_try_oauth_login_skips_when_api_key_or_unsupported() {
    use rho_harness_core::provider::ProviderId;

    let config = Config::default();
    let temp_auth = rho_test_support::TestFile::new();
    let mut auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();

    let res = super::try_oauth_login(ProviderId::OpenAi, &config, &mut auth_store)
        .await
        .unwrap();
    assert!(!res);
}

#[test]
fn test_parse_selection_choice() {
    use super::provider::parse_selection_choice;

    assert_eq!(parse_selection_choice("1", 3).unwrap(), 0);
    assert_eq!(parse_selection_choice("3", 3).unwrap(), 2);
    assert!(parse_selection_choice("0", 3).is_err());
    assert!(parse_selection_choice("4", 3).is_err());
    assert!(parse_selection_choice("abc", 3).is_err());
}

#[test]
fn test_prompt_select_from() {
    use super::provider::prompt_select_from;
    use std::io::Cursor;

    let items = vec!["Option A".to_string(), "Option B".to_string()];
    let mut reader = Cursor::new(b"2\n");
    let mut writer = Vec::new();
    let idx = prompt_select_from(&mut reader, &mut writer, "Pick:", &items).unwrap();
    assert_eq!(idx, 1);
    let output = String::from_utf8(writer).unwrap();
    assert!(output.contains("Pick:"));
    assert!(output.contains("1. Option A"));
    assert!(output.contains("2. Option B"));
}

#[test]
fn test_prompt_auth_choice_maps_selection_to_method() {
    use super::provider::{AuthMethod, prompt_auth_choice};
    use crate::error::AppError;

    let cases = [
        (Some(0), Some(AuthMethod::ApiKey)),
        (Some(1), Some(AuthMethod::OAuth)),
        (None, None),
    ];
    for (selected, expected) in cases {
        let select = move |prompt: &str, items: &[String]| {
            assert_eq!(prompt, "Pick:");
            assert_eq!(items, ["first".to_string(), "second".to_string()]);
            selected.ok_or_else(|| AppError::Cancelled("cancelled".to_string()))
        };
        let result = prompt_auth_choice(
            select,
            "Pick:",
            ["first", "second"],
            [AuthMethod::ApiKey, AuthMethod::OAuth],
        );
        match expected {
            Some(method) => assert_eq!(result.unwrap(), method),
            None => assert!(result.is_err()),
        }
    }
}

#[test]
fn test_prompt_auth_fns_unavailable_without_terminal() {
    use super::provider::{prompt_auth_method, prompt_select_login_provider};
    use crate::error::AppError;

    let temp_auth = rho_test_support::TestFile::new();
    let auth_store = crate::auth::AuthStore::load(temp_auth.path()).unwrap();
    let provider = prompt_select_login_provider(&Config::default(), &auth_store);
    assert!(matches!(provider, Err(AppError::Cancelled(msg)) if msg == "Interactive selection unavailable"));
    let method = prompt_auth_method("Anthropic");
    assert!(matches!(method, Err(AppError::Cancelled(msg)) if msg == "Interactive selection unavailable"));
}
