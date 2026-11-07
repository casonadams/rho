use rho_engine::auth::AuthStore;
use rho_engine::provider::{ModelHandle, ProviderFactory};
use rho_harness_core::config::Config;

fn with_test_env<T, F: FnOnce(&std::path::Path) -> T>(toml: &str, env_pair: (&str, &str), f: F) -> T {
    let (key_var, key_val) = env_pair;
    let prev_rho_home = std::env::var_os("RHO_HOME");
    let home = std::env::temp_dir().join(format!("rho_e2e_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), toml).unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", &home);
        std::env::set_var(key_var, key_val);
    }
    let result = f(&home);
    unsafe {
        std::env::remove_var(key_var);
        if let Some(prev) = prev_rho_home {
            std::env::set_var("RHO_HOME", prev);
        } else {
            std::env::remove_var("RHO_HOME");
        }
    }
    let _ = std::fs::remove_dir_all(home);
    result
}

fn create_model(
    toml: &str,
    env_pair: (&str, &str),
    model: &str,
) -> (Config, rho_harness_core::error::Result<ModelHandle>) {
    let _guard = crate::ENV_LOCK.lock().unwrap();
    with_test_env(toml, env_pair, |home| {
        let config = Config::load(None).unwrap();
        let auth_store = AuthStore::load(home.join("auth.json")).unwrap();
        let shared_auth = std::sync::Arc::new(tokio::sync::Mutex::new(auth_store.clone()));
        let handle = ProviderFactory::create_model(&config, model, &auth_store, shared_auth);
        (config, handle)
    })
}

fn provider_toml(provider: &str, model: &str, base_url: &str, key_env: &str) -> String {
    format!(
        "[models]\ndefault = \"{provider}/{model}\"\n\n[providers.{provider}]\nbase_url = \"{base_url}\"\nkey_env = \"{key_env}\"\n"
    )
}

#[test]
fn custom_provider_config_file_cases() {
    let cases = [
        (
            "acme",
            "https://api.acme.dev/v1",
            "RHO_E2E_ACME_KEY",
            "acme-secret",
            "acme-large",
            false,
        ),
        (
            "custom-private",
            "http://127.0.0.1:8080/v1",
            "RHO_E2E_LOCAL_KEY",
            "local-secret",
            "llama",
            true,
        ),
    ];
    for (provider, base_url, key_env, key_value, model, blocked) in cases {
        let toml = provider_toml(provider, model, base_url, key_env);
        let (config, handle) = create_model(&toml, (key_env, key_value), model);
        if blocked {
            let error = handle.unwrap_err();
            assert!(error.to_string().contains("blocked"), "{error}");
        } else {
            assert!(config.providers.contains_key(provider));
            assert_eq!(handle.unwrap().provider_name(), provider);
        }
    }
}
