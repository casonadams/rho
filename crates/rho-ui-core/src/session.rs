use serde::{Deserialize, Serialize};

use crate::ir::ContentBlock;
use crate::parser::StreamEvent;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthMode {
    OAuth,
    ApiKey,
    Local,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDef {
    pub id: &'static str,
    pub name: &'static str,
    pub auth_mode: AuthMode,
    pub default_env: &'static str,
    pub description: &'static str,
}

pub const PROVIDER_DEFS: &[ProviderDef] = &[
    ProviderDef {
        id: "anthropic",
        name: "Anthropic Claude",
        auth_mode: AuthMode::OAuth,
        default_env: "ANTHROPIC_API_KEY",
        description: "Sonnet, Opus, and Haiku with reasoning and prompt caching",
    },
    ProviderDef {
        id: "openai",
        name: "OpenAI",
        auth_mode: AuthMode::OAuth,
        default_env: "OPENAI_API_KEY",
        description: "GPT-4o, o1, o3-mini models",
    },
    ProviderDef {
        id: "gemini",
        name: "Google Gemini",
        auth_mode: AuthMode::ApiKey,
        default_env: "GEMINI_API_KEY",
        description: "Gemini 2.5 Pro & Flash with 1M-2M context window",
    },
    ProviderDef {
        id: "ollama",
        name: "Ollama (Local)",
        auth_mode: AuthMode::Local,
        default_env: "OLLAMA_HOST",
        description: "Locally running open models with zero egress",
    },
    ProviderDef {
        id: "openrouter",
        name: "OpenRouter",
        auth_mode: AuthMode::ApiKey,
        default_env: "OPENROUTER_API_KEY",
        description: "Multi-provider model gateway and aggregated billing",
    },
    ProviderDef {
        id: "groq",
        name: "Groq",
        auth_mode: AuthMode::ApiKey,
        default_env: "GROQ_API_KEY",
        description: "Ultra-low-latency LPU inference",
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SessionTurnState {
    #[default]
    Idle,
    Running {
        active_tool: Option<String>,
    },
    AwaitingApproval,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionCommand {
    Prompt { text: String },
    Steer { text: String },
    Abort,
    SwitchModel { model_id: String },
    SwitchThinking { level: String },
    Clear,
}

#[derive(Debug, Default)]
pub struct SessionState {
    pub session_id: String,
    pub turn_state: SessionTurnState,
    pub blocks: Vec<ContentBlock>,
    pub active_model: String,
    pub thinking_level: String,
    pub user_scroll_locked: bool,
}

impl SessionState {
    pub fn new(session_id: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            turn_state: SessionTurnState::Idle,
            blocks: Vec::new(),
            active_model: model.into(),
            thinking_level: "medium".to_string(),
            user_scroll_locked: false,
        }
    }

    pub fn handle_command(&mut self, cmd: SessionCommand) {
        match cmd {
            SessionCommand::Prompt { text } => {
                self.turn_state = SessionTurnState::Running { active_tool: None };
                self.blocks.push(ContentBlock::UserPrompt {
                    text,
                    images: Vec::new(),
                });
            }
            SessionCommand::Steer { text } => {
                if let SessionTurnState::Running { .. } = self.turn_state {
                    self.blocks.push(ContentBlock::Notice {
                        text: format!("[Steering]: {text}"),
                        is_error: false,
                    });
                }
            }
            SessionCommand::Abort => {
                self.turn_state = SessionTurnState::Idle;
                self.blocks.push(ContentBlock::Notice {
                    text: "Execution interrupted by user.".to_string(),
                    is_error: true,
                });
            }
            SessionCommand::SwitchModel { model_id } => {
                self.active_model = model_id.clone();
                self.blocks.push(ContentBlock::Notice {
                    text: format!("Switched model to {model_id}"),
                    is_error: false,
                });
            }
            SessionCommand::SwitchThinking { level } => {
                self.thinking_level = level.clone();
                self.blocks.push(ContentBlock::Notice {
                    text: format!("Thinking level set to {level}"),
                    is_error: false,
                });
            }
            SessionCommand::Clear => {
                self.blocks.clear();
                self.turn_state = SessionTurnState::Idle;
            }
        }
    }

    pub fn apply_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::Token(token) => {
                if let Some(ContentBlock::Paragraph(inlines)) = self.blocks.last_mut() {
                    inlines.push(crate::ir::InlineSpan::Text(token));
                } else {
                    self.blocks
                        .push(ContentBlock::Paragraph(vec![crate::ir::InlineSpan::Text(token)]));
                }
            }
            StreamEvent::ThinkingStarted => {
                self.blocks.push(ContentBlock::Thinking {
                    content: String::new(),
                    is_complete: false,
                    duration_ms: None,
                });
            }
            StreamEvent::ThinkingDelta(delta) => {
                if let Some(ContentBlock::Thinking { content, .. }) = self.blocks.last_mut() {
                    content.push_str(&delta);
                }
            }
            StreamEvent::ThinkingFinished { duration_ms } => {
                if let Some(ContentBlock::Thinking {
                    is_complete,
                    duration_ms: d,
                    ..
                }) = self.blocks.last_mut()
                {
                    *is_complete = true;
                    *d = duration_ms;
                }
            }
            StreamEvent::ToolStarted(inv) => {
                self.turn_state = SessionTurnState::Running {
                    active_tool: Some(inv.name.clone()),
                };
                self.blocks.push(ContentBlock::ToolCall(inv));
            }
            StreamEvent::ToolDelta { chunk, .. } => {
                if let Some(ContentBlock::ToolResult { output, .. }) = self.blocks.last_mut() {
                    output.push_str(&chunk);
                }
            }
            StreamEvent::ToolFinished {
                id,
                is_error,
                output,
                duration_ms,
            } => {
                self.turn_state = SessionTurnState::Running { active_tool: None };
                let inv = if let Some(ContentBlock::ToolCall(call)) = self.blocks.last() {
                    call.clone()
                } else {
                    crate::ir::ToolInvocation {
                        id,
                        name: "tool".to_string(),
                        args_summary: String::new(),
                        raw_args: None,
                    }
                };
                self.blocks.push(ContentBlock::ToolResult {
                    invocation: inv,
                    output,
                    is_error,
                    duration_ms,
                    images: Vec::new(),
                });
            }
            StreamEvent::Notice(text) => {
                self.blocks.push(ContentBlock::Notice { text, is_error: false });
            }
            StreamEvent::StatusChanged(_) | StreamEvent::UsageUpdate { .. } => {}
            StreamEvent::TurnCompleted => {
                self.turn_state = SessionTurnState::Idle;
            }
        }
    }

    pub fn set_user_scroll_lock(&mut self, locked: bool) {
        self.user_scroll_locked = locked;
    }
}
