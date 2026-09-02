use super::completion::ModelItem;
use rho_core::config::Config;
use rho_engine::auth::AuthStore;

/// Well-known model presets with real context windows and capabilities.
pub const STANDARD_PRESETS: &[(&str, &str, &str)] = &[
    ("claude-3-7-sonnet-20250219", "anthropic", "200k ctx · reasoning"),
    ("claude-3-5-sonnet-20241022", "anthropic", "200k ctx · hybrid"),
    ("claude-3-5-haiku-20241022", "anthropic", "200k ctx · fast"),
    ("gpt-4o", "openai", "128k ctx · multimodal"),
    ("gpt-4o-mini", "openai", "128k ctx · fast"),
    ("o1", "openai", "200k ctx · deep reasoning"),
    ("o3-mini", "openai", "200k ctx · reasoning"),
    ("gemini-2.5-pro", "gemini", "2M ctx · reasoning"),
    ("gemini-2.0-flash", "gemini", "1M ctx · fast"),
    ("deepseek-chat", "deepseek", "64k ctx · fast"),
    ("deepseek-reasoner", "deepseek", "64k ctx · reasoning"),
    ("llama-3.3-70b-versatile", "groq", "128k ctx · fast"),
    ("mistral-large-latest", "mistral", "128k ctx · general"),
    ("command-r-plus", "cohere", "128k ctx · search/rag"),
];

/// Dynamically discovers models available to the current user.
pub fn discover_models(config: &Config, auth_store: &AuthStore) -> Vec<ModelItem> {
    let mut models = Vec::new();

    // 1. Current active model always first if not already covered
    let active_ctx = rho_core::tokens::context_window_size(&config.model);
    let active_ctx_str = if active_ctx >= 1_000_000 {
        format!("{}M ctx", active_ctx / 1_000_000)
    } else {
        format!("{}k ctx", active_ctx / 1000)
    };
    models.push(ModelItem {
        id: config.model.clone(),
        provider: config.provider.clone(),
        description: format!("{active_ctx_str} · active"),
    });

    // 2. Custom configured providers from config.toml ([providers.<name>])
    for (name, spec) in &config.providers {
        if name != &config.provider {
            models.push(ModelItem {
                id: format!("{name}-default"),
                provider: name.clone(),
                description: format!("endpoint: {}", spec.base_url),
            });
        }
    }

    // 3. Models from authenticated/configured providers
    let configured_providers = auth_store.list_configured_providers();
    for &(model_id, provider, desc) in STANDARD_PRESETS {
        let is_configured =
            configured_providers.iter().any(|p| p == provider) || provider == "ollama" || provider == config.provider;

        if is_configured && !models.iter().any(|m| m.id == model_id) {
            models.push(ModelItem {
                id: model_id.to_string(),
                provider: provider.to_string(),
                description: desc.to_string(),
            });
        }
    }

    // 4. If few models are configured, offer standard presets so users can discover options
    if models.len() <= 3 {
        for &(model_id, provider, desc) in STANDARD_PRESETS {
            if !models.iter().any(|m| m.id == model_id) {
                models.push(ModelItem {
                    id: model_id.to_string(),
                    provider: provider.to_string(),
                    description: format!("{desc} [requires login]"),
                });
            }
        }
    }

    models
}
