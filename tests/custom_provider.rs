use rho_engine::auth::AuthStore;
use rho_engine::provider::{ModelCatalog, ModelHandle, ModelRouter};
use rho_harness_core::config::Config;
use test_case::test_case;

fn with_test_env<T, F: FnOnce(&std::path::Path) -> T>(toml: &str, env_pair: (&'static str, &str), f: F) -> T {
    let (key_var, key_val) = env_pair;
    let home = rho_test_support::TestDir::with_prefix("rho_e2e_");
    std::fs::write(home.join("config.toml"), toml).unwrap();
    let _env = rho_test_support::EnvVarGuard::set_all(&[("RHO_HOME", home.to_str()), (key_var, Some(key_val))]);
    f(&home)
}

fn create_model(toml: &str, env_pair: (&'static str, &str)) -> (Config, rho_harness_core::error::Result<ModelHandle>) {
    with_test_env(toml, env_pair, |home| {
        let config = Config::load(None).unwrap();
        let auth_store = AuthStore::load(home.join("auth.json")).unwrap();
        let router = ModelRouter::new(
            std::sync::Arc::new(tokio::sync::Mutex::new(auth_store)),
            ModelCatalog::default(),
        );
        let handle = router.build_blocking(&config, &config.route());
        (config, handle)
    })
}

fn provider_toml(provider: &str, model: &str, base_url: &str, key_env: &str) -> String {
    format!(
        "[models]\ndefault = \"{provider}/{model}\"\n\n[providers.{provider}]\nbase_url = \"{base_url}\"\nkey_env = \"{key_env}\"\n"
    )
}

#[test_case("acme", "https://api.acme.dev/v1", "RHO_E2E_ACME_KEY", "acme-secret", "acme-large", false; "public acme")]
#[test_case("custom-private", "http://127.0.0.1:8080/v1", "RHO_E2E_LOCAL_KEY", "local-secret", "llama", true; "private loopback blocked")]
fn custom_provider_config_file_cases(
    provider: &str,
    base_url: &str,
    key_env: &'static str,
    key_value: &str,
    model: &str,
    blocked: bool,
) {
    let toml = provider_toml(provider, model, base_url, key_env);
    let (config, handle) = create_model(&toml, (key_env, key_value));
    if blocked {
        let error = handle.unwrap_err();
        assert!(error.to_string().contains("blocked"), "{error}");
    } else {
        assert!(config.providers.contains_key(provider));
        assert_eq!(handle.unwrap().provider_name(), provider);
    }
}
