//! Static preset model catalogs and context descriptions for supported providers.

use super::DiscoveredModel;

pub(crate) fn format_context_desc(model_id: &str) -> String {
    format_context_tokens(rho_harness_core::tokens::context_window_size(model_id))
}

pub fn format_context_tokens(ctx: usize) -> String {
    if ctx >= 1_000_000 {
        format!("{}M ctx", ctx / 1_000_000)
    } else {
        format!("{}k ctx", ctx / 1000)
    }
}

pub fn antigravity_preset_models() -> Vec<DiscoveredModel> {
    super::antigravity::sort_models_newest_first(vec![
        DiscoveredModel {
            context_tokens: None,
            id: "gemini-3.8-flash".into(),
            name: "Gemini 3.8 Flash".into(),
            provider: "antigravity".into(),
            description: "1M ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gemini-3.7-flash".into(),
            name: "Gemini 3.7 Flash".into(),
            provider: "antigravity".into(),
            description: "1M ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gemini-3.1-pro".into(),
            name: "Gemini 3.1 Pro".into(),
            provider: "antigravity".into(),
            description: "1M ctx · reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "claude-sonnet-4-6".into(),
            name: "Claude Sonnet 4.6".into(),
            provider: "antigravity".into(),
            description: "200k ctx · reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "claude-opus-4-6".into(),
            name: "Claude Opus 4.6".into(),
            provider: "antigravity".into(),
            description: "250k ctx · deep reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-oss-120b".into(),
            name: "GPT-OSS 120B".into(),
            provider: "antigravity".into(),
            description: "128k ctx · open".into(),
        },
    ])
}

pub fn chatgpt_codex_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.4".into(),
            name: "GPT-5.4".into(),
            provider: "chatgpt".into(),
            description: "272k ctx · reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.4-pro".into(),
            name: "GPT-5.4 Pro".into(),
            provider: "chatgpt".into(),
            description: "272k ctx · deep reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.3-codex".into(),
            name: "GPT-5.3 Codex".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · coding".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.3-codex-spark".into(),
            name: "GPT-5.3 Codex Spark".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · ultra-fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.3-instant".into(),
            name: "GPT-5.3 Instant".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.6-luna".into(),
            name: "GPT-5.6 Luna".into(),
            provider: "chatgpt".into(),
            description: "372k ctx · fast reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.6-terra".into(),
            name: "GPT-5.6 Terra".into(),
            provider: "chatgpt".into(),
            description: "372k ctx · balanced reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-5.6-sol".into(),
            name: "GPT-5.6 Sol".into(),
            provider: "chatgpt".into(),
            description: "372k ctx · deep reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "chatgpt".into(),
            description: "128k ctx".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-4o-mini".into(),
            name: "GPT-4o mini".into(),
            provider: "chatgpt".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "o1".into(),
            name: "o1".into(),
            provider: "chatgpt".into(),
            description: "200k ctx · reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
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
            context_tokens: None,
            id: "gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "copilot".into(),
            description: "128k ctx".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "claude-3.5-sonnet".into(),
            name: "Claude 3.5 Sonnet".into(),
            provider: "copilot".into(),
            description: "200k ctx".into(),
        },
        DiscoveredModel {
            context_tokens: None,
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
            context_tokens: None,
            id: "claude-3-7-sonnet-20250219".into(),
            name: "Claude 3.7 Sonnet".into(),
            provider: "anthropic".into(),
            description: "200k ctx · reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "claude-3-5-sonnet-20241022".into(),
            name: "Claude 3.5 Sonnet".into(),
            provider: "anthropic".into(),
            description: "200k ctx · hybrid".into(),
        },
        DiscoveredModel {
            context_tokens: None,
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
            context_tokens: None,
            id: "gpt-4o".into(),
            name: "GPT-4o".into(),
            provider: "openai".into(),
            description: "128k ctx · multimodal".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "gpt-4o-mini".into(),
            name: "GPT-4o mini".into(),
            provider: "openai".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "o1".into(),
            name: "o1".into(),
            provider: "openai".into(),
            description: "200k ctx · deep reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
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
            context_tokens: None,
            id: "gemini-2.0-flash".into(),
            name: "Gemini 2.0 Flash".into(),
            provider: "gemini".into(),
            description: "1M ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
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
            context_tokens: None,
            id: "deepseek-chat".into(),
            name: "DeepSeek V3".into(),
            provider: "deepseek".into(),
            description: "64k ctx · general".into(),
        },
        DiscoveredModel {
            context_tokens: None,
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
            context_tokens: None,
            id: "llama-3.3-70b-versatile".into(),
            name: "Llama 3.3 70B".into(),
            provider: "groq".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
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
            context_tokens: None,
            id: "anthropic/claude-3.7-sonnet".into(),
            name: "Claude 3.7 Sonnet".into(),
            provider: "openrouter".into(),
            description: "200k ctx · reasoning".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "deepseek/deepseek-r1".into(),
            name: "DeepSeek R1".into(),
            provider: "openrouter".into(),
            description: "64k ctx · reasoning".into(),
        },
    ]
}

pub fn mistral_preset_models() -> Vec<DiscoveredModel> {
    vec![DiscoveredModel {
        context_tokens: None,
        id: "mistral-large-latest".into(),
        name: "Mistral Large".into(),
        provider: "mistral".into(),
        description: "128k ctx · general".into(),
    }]
}

pub fn xai_preset_models() -> Vec<DiscoveredModel> {
    vec![DiscoveredModel {
        context_tokens: None,
        id: "grok-2-latest".into(),
        name: "Grok 2".into(),
        provider: "xai".into(),
        description: "128k ctx".into(),
    }]
}

pub fn cohere_preset_models() -> Vec<DiscoveredModel> {
    vec![DiscoveredModel {
        context_tokens: None,
        id: "command-r-plus".into(),
        name: "Command R+".into(),
        provider: "cohere".into(),
        description: "128k ctx · search/rag".into(),
    }]
}

pub fn ollama_cloud_preset_models() -> Vec<DiscoveredModel> {
    vec![
        DiscoveredModel {
            context_tokens: None,
            id: "glm-5.3-flash".into(),
            name: "GLM 5.3 Flash".into(),
            provider: "ollama-cloud".into(),
            description: "128k ctx · fast".into(),
        },
        DiscoveredModel {
            context_tokens: None,
            id: "llama-3.3-70b".into(),
            name: "Llama 3.3 70B".into(),
            provider: "ollama-cloud".into(),
            description: "128k ctx · general".into(),
        },
    ]
}

pub(crate) fn default_presets_for(provider: &str) -> Vec<DiscoveredModel> {
    match provider {
        "chatgpt" => chatgpt_codex_models(),
        "openai" => openai_preset_models(),
        "anthropic" => anthropic_preset_models(),
        "gemini" => gemini_preset_models(),
        "antigravity" => antigravity_preset_models(),
        "deepseek" => deepseek_preset_models(),
        "groq" => groq_preset_models(),
        "openrouter" => openrouter_preset_models(),
        "mistral" => mistral_preset_models(),
        "xai" => xai_preset_models(),
        "cohere" => cohere_preset_models(),
        "ollama-cloud" => ollama_cloud_preset_models(),
        _ => vec![DiscoveredModel {
            context_tokens: None,
            id: format!("{provider}-default"),
            name: format!("{provider} Model"),
            provider: provider.to_string(),
            description: "custom model".to_string(),
        }],
    }
}
