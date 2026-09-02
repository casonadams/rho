use rho_core::config::Config;
use rho_engine::auth::AuthStore;
use rho_engine::provider::ProviderFactory;

// Tests in this binary mutate process env (RHO_HOME); serialize them.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// End-to-end: a provider defined only in config.toml is creatable at runtime
/// with no source change (REQ-001), gated by the private-network guard (REQ-003).
#[test]
fn custom_provider_end_to_end_via_config_file() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = std::env::temp_dir().join(format!("rho_e2e_provider_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join("config.toml"),
        r#"
model = "acme-large"
provider = "acme"

[providers.acme]
base_url = "https://api.acme.dev/v1"
key_env = "RHO_E2E_ACME_KEY"
"#,
    )
    .unwrap();

    // RHO_HOME only affects dir resolution in this file's Config::load call.
    unsafe {
        std::env::set_var("RHO_HOME", &home);
        std::env::set_var("RHO_E2E_ACME_KEY", "acme-secret");
    }

    let config = Config::load(None).unwrap();
    assert!(config.providers.contains_key("acme"));

    let auth_dir = home.join("auth");
    std::fs::create_dir_all(&auth_dir).unwrap();
    let auth_store = AuthStore::load(auth_dir.join("auth.json")).unwrap();
    let handle = ProviderFactory::create_model(&config, "acme-large", &auth_store).unwrap();
    assert_eq!(handle.label(), Some("acme"));

    unsafe {
        std::env::remove_var("RHO_E2E_ACME_KEY");
        std::env::remove_var("RHO_HOME");
    }
    std::fs::remove_dir_all(home).unwrap();
}

#[test]
fn custom_provider_private_endpoint_blocked_by_default() {
    let _guard = ENV_LOCK.lock().unwrap();
    let home = std::env::temp_dir().join(format!("rho_e2e_provider_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&home).unwrap();
    unsafe {
        std::env::set_var("RHO_HOME", &home);
        std::env::set_var("RHO_E2E_LOCAL_KEY", "local-secret");
    }
    std::fs::write(
        home.join("config.toml"),
        r#"
provider = "local"

[providers.local]
base_url = "http://127.0.0.1:8080/v1"
key_env = "RHO_E2E_LOCAL_KEY"
"#,
    )
    .unwrap();

    let config = Config::load(None).unwrap();
    let auth_store = AuthStore::load(home.join("auth.json")).unwrap();

    let error = ProviderFactory::create_model(&config, "llama", &auth_store).unwrap_err();
    assert!(error.to_string().contains("blocked"), "{error}");

    unsafe {
        std::env::remove_var("RHO_E2E_LOCAL_KEY");
        std::env::remove_var("RHO_HOME");
    }
    std::fs::remove_dir_all(home).unwrap();
}
