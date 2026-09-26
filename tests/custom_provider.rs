use rho_engine::auth::AuthStore;
use rho_engine::provider::ProviderFactory;
use rho_harness_core::config::Config;

fn with_test_env<F: FnOnce(&std::path::Path)>(toml: &str, env_pair: (&str, &str), f: F) {
    let (key_var, key_val) = env_pair;
    let prev_rho_home = std::env::var_os("RHO_HOME");
    let home = std::env::temp_dir().join(format!("rho_e2e_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join("config.toml"), toml).unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", &home);
        std::env::set_var(key_var, key_val);
    }
    f(&home);
    unsafe {
        std::env::remove_var(key_var);
        if let Some(prev) = prev_rho_home {
            std::env::set_var("RHO_HOME", prev);
        } else {
            std::env::remove_var("RHO_HOME");
        }
    }
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn custom_provider_end_to_end_via_config_file() {
    let _guard = crate::ENV_LOCK.lock().unwrap();
    let toml = "[models]\ndefault = \"acme/acme-large\"\n\n[providers.acme]\nbase_url = \"https://api.acme.dev/v1\"\nkey_env = \"RHO_E2E_ACME_KEY\"\n";
    with_test_env(toml, ("RHO_E2E_ACME_KEY", "acme-secret"), |home| {
        let config = Config::load(None).unwrap();
        assert!(config.providers.contains_key("acme"));
        let auth_store = AuthStore::load(home.join("auth.json")).unwrap();
        let handle = ProviderFactory::create_model(&config, "acme-large", &auth_store).unwrap();
        assert_eq!(handle.label(), Some("acme"));
    });
}

#[test]
fn custom_provider_private_endpoint_blocked_by_default() {
    let _guard = crate::ENV_LOCK.lock().unwrap();
    let toml = "[models]\ndefault = \"custom-private/llama\"\n\n[providers.custom-private]\nbase_url = \"http://127.0.0.1:8080/v1\"\nkey_env = \"RHO_E2E_LOCAL_KEY\"\n";
    with_test_env(toml, ("RHO_E2E_LOCAL_KEY", "local-secret"), |home| {
        let config = Config::load(None).unwrap();
        let auth_store = AuthStore::load(home.join("auth.json")).unwrap();
        let error = ProviderFactory::create_model(&config, "llama", &auth_store).unwrap_err();
        assert!(error.to_string().contains("blocked"), "{error}");
    });
}
