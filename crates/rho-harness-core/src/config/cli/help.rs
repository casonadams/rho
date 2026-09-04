pub const CLI_AFTER_HELP: &str = "\
Environment Variables:
  ANTHROPIC_API_KEY                - Anthropic Claude API key
  OPENAI_API_KEY                   - OpenAI GPT API key
  DEEPSEEK_API_KEY                 - DeepSeek API key
  GEMINI_API_KEY                   - Google Gemini API key
  GROQ_API_KEY                     - Groq API key
  XAI_API_KEY                      - xAI Grok API key
  OPENROUTER_API_KEY               - OpenRouter API key
  MISTRAL_API_KEY                  - Mistral API key
  COHERE_API_KEY                   - Cohere API key
  OLLAMA_API_KEY                   - Ollama Cloud API key
  OLLAMA_HOST                      - Ollama service host (default: http://localhost:11434)
  AI_MODEL                         - Default model ID
  AI_PROVIDER                      - Default provider
  AI_THINKING_LEVEL                - Default thinking level (off, minimal, low, medium, high, xhigh, max)
  AI_MAX_OUTPUT_TOKENS             - Maximum output tokens per turn
  AI_MAX_TURNS                     - Maximum model turns per run (default: 250)
  AI_CONTEXT_WINDOW_MESSAGES       - Context window messages before compaction (default: 24)
  AI_COMPACTION_MAX_BYTES          - Maximum bytes per compaction summary (default: 8192)
  RHO_HOME                         - Custom configuration directory (default: ~/.config/rho)

Authentication:
  OAuth or API key: openrouter
  API key: anthropic, openai, deepseek, gemini, groq, xai, mistral, cohere, ollama-cloud
  Subscription OAuth: chatgpt, copilot, antigravity (explicit login required via 'rho login <provider>')
  Local: local (no login required; customize with OLLAMA_HOST)
  Custom: Any OpenAI-compatible endpoint configured in config.toml under [providers.<name>]";
