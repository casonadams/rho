//! Live dynamic model discovery from authenticated provider endpoints.

use crate::auth::AuthStore;
use rho_harness_core::error::Result;
use rho_harness_core::provider::ProviderId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub description: String,
}

pub async fn discover_provider_models(provider: ProviderId, auth_store: &AuthStore) -> Result<Vec<DiscoveredModel>> {
    match provider {
        ProviderId::ChatGpt => Ok(chatgpt_codex_models()),
        ProviderId::Copilot => Ok(copilot_models()),
        ProviderId::Local => discover_ollama_models().await,
        ProviderId::OpenAi => {
            if let Some(key) = auth_store.get_key_sync("openai")? {
                discover_openai_compatible("openai", "https://api.openai.com/v1", &key).await
            } else {
                Ok(openai_preset_models())
            }
        }
        ProviderId::OpenRouter => {
            if let Some(key) = auth_store.get_key_sync("openrouter")? {
                discover_openai_compatible("openrouter", "https://openrouter.ai/api/v1", &key).await
            } else {
                Ok(openrouter_preset_models())
            }
        }
        ProviderId::Groq => {
            if let Some(key) = auth_store.get_key_sync("groq")? {
                discover_openai_compatible("groq", "https://api.groq.com/openai/v1", &key).await
            } else {
                Ok(groq_preset_models())
            }
        }
        ProviderId::DeepSeek => {
            if let Some(key) = auth_store.get_key_sync("deepseek")? {
                discover_openai_compatible("deepseek", "https://api.deepseek.com", &key).await
            } else {
                Ok(deepseek_preset_models())
            }
        }
        ProviderId::Anthropic => {
            if let Some(key) = auth_store.get_key_sync("anthropic")? {
                discover_anthropic_models(&key).await
            } else {
                Ok(anthropic_preset_models())
            }
        }
        ProviderId::Gemini | ProviderId::Antigravity => {
            if let Some(key) = auth_store.get_key_sync("gemini")? {
                discover_gemini_models(&key).await
            } else {
                Ok(gemini_preset_models())
            }
        }
        ProviderId::Mistral => Ok(mistral_preset_models()),
        ProviderId::XAi => Ok(xai_preset_models()),
        ProviderId::Cohere => Ok(cohere_preset_models()),
        ProviderId::OllamaCloud => {
            if let Some(key) = auth_store.get_key_sync("ollama-cloud")? {
                discover_openai_compatible("ollama-cloud", "https://ollama.com/v1", &key).await
            } else {
                Ok(ollama_cloud_preset_models())
            }
        }
    }
}

pub async fn discover_custom_provider_models(
    name: &str,
    base_url: &str,
    api_key: Option<&str>,
) -> Result<Vec<DiscoveredModel>> {
    let key = api_key.unwrap_or_default();
    discover_openai_compatible(name, base_url, key).await
}

async fn discover_openai_compatible(
    provider_name: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<DiscoveredModel>> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();

    let endpoint = format!("{}/models", base_url.trim_end_matches('/'));
    let mut req = client.get(&endpoint);
    if !api_key.trim().is_empty() {
        req = req.header("Authorization", format!("Bearer {}", api_key.trim()));
    }

    if let Ok(resp) = req.send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<OpenAiModelsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.data {
            let desc = format_context_desc(&item.id);
            models.push(DiscoveredModel {
                id: item.id.clone(),
                name: item.id.clone(),
                provider: provider_name.to_string(),
                description: desc,
            });
        }
        if !models.is_empty() {
            models.sort_by(|a, b| a.id.cmp(&b.id));
            return Ok(models);
        }
    }

    Ok(default_presets_for(provider_name))
}

async fn discover_ollama_models() -> Result<Vec<DiscoveredModel>> {
    let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .no_proxy()
        .build()
        .unwrap_or_default();
    let endpoint = format!("{}/api/tags", host.trim_end_matches('/'));

    if let Ok(resp) = client.get(&endpoint).send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<OllamaTagsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.models {
            let id = item.name;
            models.push(DiscoveredModel {
                name: id.clone(),
                id,
                provider: "local".to_string(),
                description: "local model".to_string(),
            });
        }
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(Vec::new())
}

async fn discover_anthropic_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();

    if let Ok(resp) = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<AnthropicModelsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.data {
            let desc = format_context_desc(&item.id);
            models.push(DiscoveredModel {
                id: item.id.clone(),
                name: item.display_name.unwrap_or_else(|| item.id.clone()),
                provider: "anthropic".to_string(),
                description: desc,
            });
        }
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(anthropic_preset_models())
}

async fn discover_gemini_models(api_key: &str) -> Result<Vec<DiscoveredModel>> {
    let client = reqwest::Client::builder().no_proxy().build().unwrap_or_default();

    let endpoint = format!(
        "https://generativelanguage.googleapis.com/v1beta/models?key={}",
        api_key.trim()
    );
    if let Ok(resp) = client.get(&endpoint).send().await
        && resp.status().is_success()
        && let Ok(body) = resp.json::<GeminiModelsResponse>().await
    {
        let mut models = Vec::new();
        for item in body.models {
            let id = item.name.strip_prefix("models/").unwrap_or(&item.name);
            if id.starts_with("gemini") {
                models.push(DiscoveredModel {
                    id: id.to_string(),
                    name: item.display_name.unwrap_or_else(|| id.to_string()),
                    provider: "gemini".to_string(),
                    description: format_context_desc(id),
                });
            }
        }
        if !models.is_empty() {
            return Ok(models);
        }
    }

    Ok(gemini_preset_models())
}

fn format_context_desc(model_id: &str) -> String {
    let ctx = rho_harness_core::tokens::context_window_size(model_id);
    if ctx >= 1_000_000 {
        format!("{}M ctx", ctx / 1_000_000)
    } else {
        format!("{}k ctx", ctx / 1000)
    }
}

pub fn chatgpt_codex_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "gpt-5.4".into(),
            name: "GPT-5.4".into(),
            provider: "chatgpt".into(),
            description: "272k ctx · reasoning".into(),
        },
        DiscoveredModel {
            id: "gpt-5.4-pro".into(),
            name: "GPT-5.4 Pro".into(),
            provider: "chatgpt".into(),
            description: "272k ctx · deep reasoning".into(),
        },
        DiscoveredModel {
            id: "gpt-5.3-codex".into(),
            name: "GPT-5.3 Codex".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · coding".into(),
        },
        DiscoveredModel {
            id: "gpt-5.3-codex-spark".into(),
            name: "GPT-5.3 Codex Spark".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · ultra-fast".into(),
        },
        DiscoveredModel {
            id: "gpt-5.3-instant".into(),
            name: "GPT-5.3 Instant".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            id: "gpt-5.6-luna".into(),
            name: "GPT-5.6 Luna".into(),
            provider: "chatgpt".into(),
            description: "372k ctx · fast reasoning".into(),
        },
        DiscoveredModel {
            id: "gpt-5.6-terra".into(),
            name: "GPT-5.6 Terra".into(),
            provider: "chatgpt".into(),
            description: "372k ctx · balanced reasoning".into(),
        },
        DiscoveredModel {
            id: "gpt-5.6-sol".into(),
            name: "GPT-5.6 Sol".into(),
            provider: "chatgpt".into(),
            description: "372k ctx · deep reasoning".into(),
        },
        DiscoveredModel {
            id: "gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "chatgpt".into(),
            description: "128k ctx".into(),
        },
        DiscoveredModel {
            id: "gpt-4o-mini".into(),
            name: "GPT-4o mini".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            id: "o1".into(),
            name: "o1".into(),
            provider: "chatgpt".into(),
            description: "200k ctx · reasoning".into(),
        },
        DiscoveredModel {
            id: "o3-mini".into(),
            name: "o3-mini".into(),
            provider: "chatgpt".into(),
            description: "200k ctx · reasoning".into(),
        },
    ]
}

pub fn copilot_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "copilot".into(),
            description: "128k ctx".into(),
        },
        DiscoveredModel {
            id: "claude-3.5-sonnet".into(),
            name: "Claude 3.5 Sonnet".into(),
            provider: "copilot".into(),
            description: "200k ctx".into(),
        },
        DiscoveredModel {
            id: "o1".into(),
            name: "o1".into(),
            provider: "copilot".into(),
            description: "200k ctx".into(),
        },
    ]
}

pub fn anthropic_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "claude-3-7-sonnet-20250219".into(),
            name: "Claude 3.7 Sonnet".into(),
            provider: "anthropic".into(),
            description: "200k ctx · reasoning".into(),
        },
        DiscoveredModel {
            id: "claude-3-5-sonnet-20241022".into(),
            name: "Claude 3.5 Sonnet".into(),
            provider: "anthropic".into(),
            description: "200k ctx · hybrid".into(),
        },
        DiscoveredModel {
            id: "claude-3-5-haiku-20241022".into(),
            name: "Claude 3.5 Haiku".into(),
            provider: "anthropic".into(),
            description: "200k ctx · fast".into(),
        },
    ]
}

pub fn openai_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "openai".into(),
            description: "128k ctx · multimodal".into(),
        },
        DiscoveredModel {
            id: "gpt-4o-mini".into(),
            name: "GPT-4o mini".into(),
            provider: "openai".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            id: "o1".into(),
            name: "o1".into(),
            provider: "openai".into(),
            description: "200k ctx · deep reasoning".into(),
        },
        DiscoveredModel {
            id: "o3-mini".into(),
            name: "o3-mini".into(),
            provider: "openai".into(),
            description: "200k ctx · reasoning".into(),
        },
    ]
}

pub fn gemini_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "gemini-2.0-flash".into(),
            name: "Gemini 2.0 Flash".into(),
            provider: "gemini".into(),
            description: "1M ctx · fast".into(),
        },
        DiscoveredModel {
            id: "gemini-1.5-pro".into(),
            name: "Gemini 1.5 Pro".into(),
            provider: "gemini".into(),
            description: "2M ctx · reasoning".into(),
        },
    ]
}

pub fn deepseek_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "deepseek-chat".into(),
            name: "DeepSeek V3".into(),
            provider: "deepseek".into(),
            description: "64k ctx · general".into(),
        },
        DiscoveredModel {
            id: "deepseek-reasoner".into(),
            name: "DeepSeek R1".into(),
            provider: "deepseek".into(),
            description: "64k ctx · reasoning".into(),
        },
    ]
}

pub fn groq_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "llama-3.3-70b-versatile".into(),
            name: "Llama 3.3 70B".into(),
            provider: "groq".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            id: "qwen-2.5-coder-32b".into(),
            name: "Qwen 2.5 Coder 32B".into(),
            provider: "groq".into(),
            description: "128k ctx · coding".into(),
        },
    ]
}

pub fn openrouter_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "anthropic/claude-3.7-sonnet".into(),
            name: "Claude 3.7 Sonnet".into(),
            provider: "openrouter".into(),
            description: "200k ctx · reasoning".into(),
        },
        DiscoveredModel {
            id: "deepseek/deepseek-r1".into(),
            name: "DeepSeek R1".into(),
            provider: "openrouter".into(),
            description: "64k ctx · reasoning".into(),
        },
    ]
}

pub fn mistral_preset_models() -> Vec<DiscoveredModel> {
    vec![DiscoveredModel {
        id: "mistral-large-latest".into(),
        name: "Mistral Large".into(),
        provider: "mistral".into(),
        description: "128k ctx · general".into(),
    }]
}

pub fn xai_preset_models() -> Vec<DiscoveredModel> {
    vec![DiscoveredModel {
        id: "grok-2-latest".into(),
        name: "Grok 2".into(),
        provider: "xai".into(),
        description: "128k ctx".into(),
    }]
}

pub fn cohere_preset_models() -> Vec<DiscoveredModel> {
    vec![DiscoveredModel {
        id: "command-r-plus".into(),
        name: "Command R+".into(),
        provider: "cohere".into(),
        description: "128k ctx · search/rag".into(),
    }]
}

pub fn ollama_cloud_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            id: "glm-5.3-flash".into(),
            name: "GLM 5.3 Flash".into(),
            provider: "ollama-cloud".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            id: "llama-3.3-70b".into(),
            name: "Llama 3.3 70B".into(),
            provider: "ollama-cloud".into(),
            description: "128k ctx · general".into(),
        },
    ]
}

fn default_presets_for(provider: &str) -> Vec<DiscoveredModel> {
    match provider {
        "chatgpt" => chatgpt_codex_models(),
        "openai" => openai_preset_models(),
        "anthropic" => anthropic_preset_models(),
        "gemini" => gemini_preset_models(),
        "deepseek" => deepseek_preset_models(),
        "groq" => groq_preset_models(),
        "openrouter" => openrouter_preset_models(),
        "mistral" => mistral_preset_models(),
        "xai" => xai_preset_models(),
        "cohere" => cohere_preset_models(),
        "ollama-cloud" => ollama_cloud_preset_models(),
        _ => vec![DiscoveredModel {
            id: format!("{provider}-default"),
            name: format!("{provider} Model"),
            provider: provider.to_string(),
            description: "custom model".to_string(),
        }],
    }
}

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelItem>,
}

#[derive(Deserialize)]
struct OpenAiModelItem {
    id: String,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagItem>,
}

#[derive(Deserialize)]
struct OllamaTagItem {
    name: String,
}

#[derive(Deserialize)]
struct AnthropicModelsResponse {
    data: Vec<AnthropicModelItem>,
}

#[derive(Deserialize)]
struct AnthropicModelItem {
    id: String,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModelItem>,
}

#[derive(Deserialize)]
struct GeminiModelItem {
    name: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}
