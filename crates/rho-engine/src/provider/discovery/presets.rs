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

fn make_preset(id: &str, name: &str, provider: &str, desc: &str) -> DiscoveredModel {
    DiscoveredModel {
        context_tokens: None,
        id: id.to_string(),
        name: name.to_string(),
        provider: provider.to_string(),
        description: desc.to_string(),
    }
}

fn build_presets(table: &[(&str, &str, &str, &str)]) -> Vec<DiscoveredModel> {
    table
        .iter()
        .map(|&(id, name, provider, desc)| make_preset(id, name, provider, desc))
        .collect()
}

const ANTIGRAVITY_PRESETS: &[(&str, &str, &str, &str)] = &[
    ("gemini-3.8-flash", "Gemini 3.8 Flash", "antigravity", "1M ctx · fast"),
    ("gemini-3.7-flash", "Gemini 3.7 Flash", "antigravity", "1M ctx · fast"),
    ("gemini-3.1-pro", "Gemini 3.1 Pro", "antigravity", "1M ctx · reasoning"),
    (
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
        "antigravity",
        "200k ctx · reasoning",
    ),
    (
        "claude-opus-4-6",
        "Claude Opus 4.6",
        "antigravity",
        "250k ctx · deep reasoning",
    ),
    ("gpt-oss-120b", "GPT-OSS 120B", "antigravity", "128k ctx · open"),
];

pub fn antigravity_preset_models() -> Vec<DiscoveredModel> {
    super::antigravity::sort_models_newest_first(build_presets(ANTIGRAVITY_PRESETS))
}

const CHATGPT_CODEX_PRESETS: &[(&str, &str, &str, &str)] = &[
    ("gpt-6-astra", "GPT-6 Astra", "chatgpt", "372k ctx · deep reasoning"),
    ("gpt-5.4", "GPT-5.4", "chatgpt", "272k ctx · reasoning"),
    ("gpt-5.4-pro", "GPT-5.4 Pro", "chatgpt", "272k ctx · deep reasoning"),
    ("gpt-5.3-codex", "GPT-5.3 Codex", "chatgpt", "128k ctx · coding"),
    (
        "gpt-5.3-codex-spark",
        "GPT-5.3 Codex Spark",
        "chatgpt",
        "128k ctx · ultra-fast",
    ),
    ("gpt-5.3-instant", "GPT-5.3 Instant", "chatgpt", "128k ctx · fast"),
    ("gpt-5.6-luna", "GPT-5.6 Luna", "chatgpt", "372k ctx · fast reasoning"),
    (
        "gpt-5.6-terra",
        "GPT-5.6 Terra",
        "chatgpt",
        "372k ctx · balanced reasoning",
    ),
    ("gpt-5.6-sol", "GPT-5.6 Sol", "chatgpt", "372k ctx · deep reasoning"),
    ("gpt-4o", "GPT-4o", "chatgpt", "128k ctx"),
    ("gpt-4o-mini", "GPT-4o mini", "chatgpt", "128k ctx · fast"),
    ("o1", "o1", "chatgpt", "200k ctx · reasoning"),
    ("o3-mini", "o3-mini", "chatgpt", "200k ctx · reasoning"),
];

pub fn chatgpt_codex_models() -> Vec<DiscoveredModel> {
    build_presets(CHATGPT_CODEX_PRESETS)
}

const COPILOT_PRESETS: &[(&str, &str, &str, &str)] = &[
    ("gpt-4o", "GPT-4o", "copilot", "128k ctx"),
    ("claude-3.5-sonnet", "Claude 3.5 Sonnet", "copilot", "200k ctx"),
    ("o1", "o1", "copilot", "200k ctx"),
];

pub fn copilot_models() -> Vec<DiscoveredModel> {
    build_presets(COPILOT_PRESETS)
}

const ANTHROPIC_PRESETS: &[(&str, &str, &str, &str)] = &[
    (
        "claude-3-7-sonnet-20250219",
        "Claude 3.7 Sonnet",
        "anthropic",
        "200k ctx · reasoning",
    ),
    (
        "claude-3-5-sonnet-20241022",
        "Claude 3.5 Sonnet",
        "anthropic",
        "200k ctx · hybrid",
    ),
    (
        "claude-3-5-haiku-20241022",
        "Claude 3.5 Haiku",
        "anthropic",
        "200k ctx · fast",
    ),
];

pub fn anthropic_preset_models() -> Vec<DiscoveredModel> {
    build_presets(ANTHROPIC_PRESETS)
}

const OPENAI_PRESETS: &[(&str, &str, &str, &str)] = &[
    ("gpt-6-astra", "GPT-6 Astra", "openai", "1.05M ctx · deep reasoning"),
    ("gpt-4o", "GPT-4o", "openai", "128k ctx · multimodal"),
    ("gpt-4o-mini", "GPT-4o mini", "openai", "128k ctx · fast"),
    ("o1", "o1", "openai", "200k ctx · deep reasoning"),
    ("o3-mini", "o3-mini", "openai", "200k ctx · reasoning"),
];

pub fn openai_preset_models() -> Vec<DiscoveredModel> {
    build_presets(OPENAI_PRESETS)
}

const GEMINI_PRESETS: &[(&str, &str, &str, &str)] = &[
    ("gemini-2.5-pro", "Gemini 2.5 Pro", "gemini", "2M ctx · reasoning"),
    ("gemini-2.5-flash", "Gemini 2.5 Flash", "gemini", "1M ctx · fast"),
    ("gemini-2.0-flash", "Gemini 2.0 Flash", "gemini", "1M ctx · fast"),
    ("gemini-1.5-pro", "Gemini 1.5 Pro", "gemini", "2M ctx · reasoning"),
];

pub fn gemini_preset_models() -> Vec<DiscoveredModel> {
    build_presets(GEMINI_PRESETS)
}

const DEEPSEEK_PRESETS: &[(&str, &str, &str, &str)] = &[
    ("deepseek-chat", "DeepSeek V3", "deepseek", "64k ctx · general"),
    ("deepseek-reasoner", "DeepSeek R1", "deepseek", "64k ctx · reasoning"),
];

pub fn deepseek_preset_models() -> Vec<DiscoveredModel> {
    build_presets(DEEPSEEK_PRESETS)
}

const GROQ_PRESETS: &[(&str, &str, &str, &str)] = &[
    ("llama-3.3-70b-versatile", "Llama 3.3 70B", "groq", "128k ctx · fast"),
    ("qwen-2.5-coder-32b", "Qwen 2.5 Coder 32B", "groq", "128k ctx · coding"),
];

pub fn groq_preset_models() -> Vec<DiscoveredModel> {
    build_presets(GROQ_PRESETS)
}

const OPENROUTER_PRESETS: &[(&str, &str, &str, &str)] = &[
    (
        "anthropic/claude-3.7-sonnet",
        "Claude 3.7 Sonnet",
        "openrouter",
        "200k ctx · reasoning",
    ),
    (
        "deepseek/deepseek-r1",
        "DeepSeek R1",
        "openrouter",
        "64k ctx · reasoning",
    ),
];

pub fn openrouter_preset_models() -> Vec<DiscoveredModel> {
    build_presets(OPENROUTER_PRESETS)
}

pub fn mistral_preset_models() -> Vec<DiscoveredModel> {
    build_presets(&[("mistral-large-latest", "Mistral Large", "mistral", "128k ctx · general")])
}

pub fn xai_preset_models() -> Vec<DiscoveredModel> {
    build_presets(&[("grok-2-latest", "Grok 2", "xai", "128k ctx")])
}

pub fn cohere_preset_models() -> Vec<DiscoveredModel> {
    build_presets(&[("command-r-plus", "Command R+", "cohere", "128k ctx · search/rag")])
}

const CLAUDE_PRESETS: &[(&str, &str, &str, &str, usize)] = &[
    (
        "claude-sonnet-4-6",
        "Claude Sonnet 4.6",
        "claude",
        "1M ctx · fast & intelligent",
        1_000_000,
    ),
    (
        "claude-opus-4-6",
        "Claude Opus 4.6",
        "claude",
        "1M ctx · deep reasoning",
        1_000_000,
    ),
    (
        "claude-haiku-4-5",
        "Claude Haiku 4.5",
        "claude",
        "200k ctx · fast",
        200_000,
    ),
    (
        "claude-sonnet-5",
        "Claude Sonnet 5",
        "claude",
        "1M ctx · frontier intelligence",
        1_000_000,
    ),
    (
        "claude-opus-5",
        "Claude Opus 5",
        "claude",
        "1M ctx · complex coding",
        1_000_000,
    ),
    (
        "claude-fable-5.1",
        "Claude Fable 5.1",
        "claude",
        "1M ctx · reasoning & agents",
        1_000_000,
    ),
];

pub fn claude_preset_models() -> Vec<DiscoveredModel> {
    CLAUDE_PRESETS
        .iter()
        .map(|&(id, name, prov, desc, tokens)| DiscoveredModel {
            context_tokens: Some(tokens),
            id: id.into(),
            name: name.into(),
            provider: prov.into(),
            description: desc.into(),
        })
        .collect()
}

const OLLAMA_CLOUD_PRESETS: &[(&str, &str, &str, &str, usize)] = &[
    (
        "glm-5.3-flash",
        "GLM 5.3 Flash",
        "ollama-cloud",
        "1M ctx · fast",
        1_048_576,
    ),
    (
        "gpt-oss:120b",
        "GPT OSS 120B",
        "ollama-cloud",
        "128k ctx · general",
        131_072,
    ),
];

pub fn ollama_cloud_preset_models() -> Vec<DiscoveredModel> {
    OLLAMA_CLOUD_PRESETS
        .iter()
        .map(|&(id, name, prov, desc, tokens)| DiscoveredModel {
            context_tokens: Some(tokens),
            id: id.into(),
            name: name.into(),
            provider: prov.into(),
            description: desc.into(),
        })
        .collect()
}

fn known_provider_presets(provider: &str) -> Option<Vec<DiscoveredModel>> {
    match provider {
        "chatgpt" => Some(chatgpt_codex_models()),
        "claude" => Some(claude_preset_models()),
        "openai" => Some(openai_preset_models()),
        "anthropic" => Some(anthropic_preset_models()),
        "gemini" => Some(gemini_preset_models()),
        "antigravity" => Some(antigravity_preset_models()),
        "deepseek" => Some(deepseek_preset_models()),
        "groq" => Some(groq_preset_models()),
        "openrouter" => Some(openrouter_preset_models()),
        "mistral" => Some(mistral_preset_models()),
        "xai" => Some(xai_preset_models()),
        "cohere" => Some(cohere_preset_models()),
        "ollama-cloud" => Some(ollama_cloud_preset_models()),
        _ => None,
    }
}

pub fn default_presets_for(provider: &str) -> Vec<DiscoveredModel> {
    if let Some(presets) = known_provider_presets(provider) {
        return presets;
    }
    match provider {
        "ollama" | "local" => vec![DiscoveredModel {
            context_tokens: Some(131_072),
            id: "llama3.2".to_string(),
            name: "Llama 3.2".to_string(),
            provider: "local".to_string(),
            description: "128k ctx · fast".to_string(),
        }],
        _ => vec![DiscoveredModel {
            context_tokens: None,
            id: format!("{provider}-default"),
            name: format!("{provider} Model"),
            provider: provider.to_string(),
            description: "custom model".to_string(),
        }],
    }
}
