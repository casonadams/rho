use crate::config::Config;
use crate::error::{AppError, Result};
use crate::plugin::capability::CapabilityId;
use crate::plugin::contract::{
    InteractionOption, InteractionRequest, InteractionResponse, NetworkAccess, OperationEffect, PathScope,
    ToolCapability, ToolDescriptor, ToolHost, ToolInvocationRequest, ToolInvocationResponse,
};
use crate::tools::ask_user::{AskUserArgs, AskUserTool, InteractiveQuestionPort, UserAnswer, UserQuestion};
use crate::tools::bash::{BashArgs, BashTool};
use crate::tools::edit::{EditArgs, EditTool};
use crate::tools::read::{ReadArgs, ReadTool};
use crate::tools::types::{ToolResult, generated_schema};
use crate::tools::web::fetch::FetchArgs;
use crate::tools::web::search::SearchArgs;
use crate::tools::web::{
    FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
};
use crate::tools::write::{WriteArgs, WriteTool};
use async_trait::async_trait;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub static PROMPT_READ: &str = include_str!("../../prompts/tools/read.md");
pub static PROMPT_WRITE: &str = include_str!("../../prompts/tools/write.md");
pub static PROMPT_EDIT: &str = include_str!("../../prompts/tools/edit.md");
pub static PROMPT_BASH: &str = include_str!("../../prompts/tools/bash.md");
pub static PROMPT_ASK_USER: &str = include_str!("../../prompts/tools/ask_user.md");
pub static PROMPT_WEBSEARCH: &str = include_str!("../../prompts/tools/websearch.md");
pub static PROMPT_WEBFETCH: &str = include_str!("../../prompts/tools/webfetch.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinToolKind {
    ReadOnly,
    WorkspaceMutation,
    Network,
    Interactive,
    Shell,
}

#[derive(Debug, Clone, Copy)]
pub struct BuiltinToolDeclaration {
    pub name: &'static str,
    pub capability: BuiltinToolKind,
    pub description: &'static str,
    pub prompt: &'static str,
    schema: fn() -> serde_json::Value,
    effects: &'static [OperationEffect],
}

const READ_EFFECTS: &[OperationEffect] = &[OperationEffect::ReadPath {
    scope: PathScope::Explicit,
}];
const WRITE_EFFECTS: &[OperationEffect] = &[OperationEffect::WritePath {
    scope: PathScope::Workspace,
}];
const BASH_EFFECTS: &[OperationEffect] = &[OperationEffect::ExecuteProcess];
const NETWORK_EFFECTS: &[OperationEffect] = &[OperationEffect::Network {
    access: NetworkAccess::PublicInternet,
}];
const INTERACTION_EFFECTS: &[OperationEffect] = &[OperationEffect::UserInteraction];

pub const DECLARATIONS: &[BuiltinToolDeclaration] = &[
    BuiltinToolDeclaration {
        name: "read",
        capability: BuiltinToolKind::ReadOnly,
        description: "Read file contents with line numbering, offset, and limit safeguards.",
        prompt: PROMPT_READ,
        schema: generated_schema::<ReadArgs>,
        effects: READ_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "write",
        capability: BuiltinToolKind::WorkspaceMutation,
        description: "Write full content to a file, automatically creating parent directories.",
        prompt: PROMPT_WRITE,
        schema: generated_schema::<WriteArgs>,
        effects: WRITE_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "edit",
        capability: BuiltinToolKind::WorkspaceMutation,
        description: "Edit a file by applying exact string replacements. Every oldText must match exactly once.",
        prompt: PROMPT_EDIT,
        schema: generated_schema::<EditArgs>,
        effects: WRITE_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "bash",
        capability: BuiltinToolKind::Shell,
        description: "Execute a shell command in the current working directory with a timeout. Do not prefix commands with cd.",
        prompt: PROMPT_BASH,
        schema: generated_schema::<BashArgs>,
        effects: BASH_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "websearch",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEBSEARCH,
        schema: generated_schema::<SearchArgs>,
        effects: NETWORK_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "web_search",
        capability: BuiltinToolKind::Network,
        description: "Search the web and return structured search results with titles, summaries, and URLs.",
        prompt: PROMPT_WEBSEARCH,
        schema: generated_schema::<SearchArgs>,
        effects: NETWORK_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "webfetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEBFETCH,
        schema: generated_schema::<FetchArgs>,
        effects: NETWORK_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "web_fetch",
        capability: BuiltinToolKind::Network,
        description: "Fetch and extract readable content from a URL (HTML, JSON, Markdown, RSS/Atom, CSV, PDF).",
        prompt: PROMPT_WEBFETCH,
        schema: generated_schema::<FetchArgs>,
        effects: NETWORK_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "ask_user",
        capability: BuiltinToolKind::Interactive,
        description: "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
        prompt: PROMPT_ASK_USER,
        schema: generated_schema::<AskUserArgs>,
        effects: INTERACTION_EFFECTS,
    },
    BuiltinToolDeclaration {
        name: "ask_user_question",
        capability: BuiltinToolKind::Interactive,
        description: "Ask the user one or more structured questions to clarify ambiguous requirements, confirm architectural choices, or gather user preferences.",
        prompt: PROMPT_ASK_USER,
        schema: generated_schema::<AskUserArgs>,
        effects: INTERACTION_EFFECTS,
    },
];

impl PartialEq for BuiltinToolDeclaration {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.capability == other.capability
            && self.description == other.description
            && self.prompt == other.prompt
            && std::ptr::fn_addr_eq(self.schema, other.schema)
            && self.effects == other.effects
    }
}

impl Eq for BuiltinToolDeclaration {}

impl BuiltinToolDeclaration {
    pub fn descriptor(self) -> ToolDescriptor {
        ToolDescriptor {
            id: format!("tool:{}", self.name).parse().unwrap(),
            description: self.description.to_string(),
            argument_schema: (self.schema)(),
            prompt_guidance: self.prompt.to_string(),
            effects: self.effects.to_vec(),
        }
    }
}

pub struct BuiltinToolCatalog {
    capabilities: BTreeMap<CapabilityId, Arc<dyn ToolCapability>>,
}

impl BuiltinToolCatalog {
    pub fn new(base_dir: &Path, config: &Config) -> Result<Self> {
        let http = HttpClient::new(config.allow_private_network)?;
        let search = WebSearchTool::new(
            http.clone(),
            SearchRateLimiter::new(config.search_min_interval_ms),
            WebSearchConfig {
                region: config.region.clone(),
                timeout_sec: config.search_timeout_sec,
            },
        );
        let fetch = WebFetchTool::new(
            http,
            FetchCache::new(60, 64),
            WebFetchConfig {
                timeout_sec: config.fetch_timeout_sec,
                max_bytes: config.fetch_max_bytes,
                default_limit: config.fetch_limit,
            },
        );
        let tools = vec![
            BuiltinTool::Read(ReadTool::new(base_dir)),
            BuiltinTool::Write(WriteTool::with_exclusions(
                base_dir,
                [&config.config_dir, &config.sessions_dir],
            )),
            BuiltinTool::Edit(EditTool::with_exclusions(
                base_dir,
                [&config.config_dir, &config.sessions_dir],
            )),
            BuiltinTool::Bash(BashTool::new(base_dir)),
            BuiltinTool::WebSearch(search.clone(), "websearch"),
            BuiltinTool::WebSearch(search, "web_search"),
            BuiltinTool::WebFetch(fetch.clone(), "webfetch"),
            BuiltinTool::WebFetch(fetch, "web_fetch"),
            BuiltinTool::AskUser(AskUserTool::new(), "ask_user"),
            BuiltinTool::AskUser(AskUserTool::new(), "ask_user_question"),
        ];
        let capabilities = tools
            .into_iter()
            .map(|tool| (tool.descriptor().id, Arc::new(tool) as Arc<dyn ToolCapability>))
            .collect();
        Ok(Self { capabilities })
    }

    pub fn descriptors() -> Vec<ToolDescriptor> {
        DECLARATIONS
            .iter()
            .copied()
            .map(BuiltinToolDeclaration::descriptor)
            .collect()
    }

    pub fn into_capabilities(self) -> BTreeMap<CapabilityId, Arc<dyn ToolCapability>> {
        self.capabilities
    }
}

enum BuiltinTool {
    Read(ReadTool),
    Write(WriteTool),
    Edit(EditTool),
    Bash(BashTool),
    WebSearch(WebSearchTool, &'static str),
    WebFetch(WebFetchTool, &'static str),
    AskUser(AskUserTool, &'static str),
}

impl BuiltinTool {
    fn name(&self) -> &'static str {
        match self {
            Self::Read(_) => "read",
            Self::Write(_) => "write",
            Self::Edit(_) => "edit",
            Self::Bash(_) => "bash",
            Self::WebSearch(_, name) => name,
            Self::WebFetch(_, name) => name,
            Self::AskUser(_, name) => name,
        }
    }
}

#[async_trait]
impl ToolCapability for BuiltinTool {
    fn descriptor(&self) -> ToolDescriptor {
        DECLARATIONS
            .iter()
            .find(|declaration| declaration.name == self.name())
            .copied()
            .unwrap()
            .descriptor()
    }

    async fn invoke(
        &self,
        host: &dyn ToolHost,
        request: ToolInvocationRequest,
    ) -> std::result::Result<ToolInvocationResponse, crate::plugin::capability::CapabilityError> {
        let result = match self {
            Self::Read(tool) => tool.execute(parse(request.arguments)?).await,
            Self::Write(tool) => tool.execute(parse(request.arguments)?).await,
            Self::Edit(tool) => tool.execute(parse(request.arguments)?).await,
            Self::Bash(tool) => tool.execute(parse(request.arguments)?).await,
            Self::WebSearch(tool, _) => tool.execute(parse(request.arguments)?).await,
            Self::WebFetch(tool, _) => tool.execute(parse(request.arguments)?).await,
            Self::AskUser(tool, _) => {
                let port = HostQuestionPort(host);
                tool.execute(&port, parse(request.arguments)?).await
            }
        }
        .map_err(map_app_error)?;
        Ok(map_result(result))
    }
}

fn parse<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> std::result::Result<T, crate::plugin::capability::CapabilityError> {
    serde_json::from_value(value).map_err(|_| crate::plugin::capability::CapabilityError::InvalidRequest {
        message: "tool arguments do not match the declared schema".to_string(),
    })
}

fn map_result(result: ToolResult) -> ToolInvocationResponse {
    ToolInvocationResponse {
        content: result.content,
        is_error: result.is_error,
        structured_content: result.metadata,
    }
}

fn map_app_error(error: AppError) -> crate::plugin::capability::CapabilityError {
    match error {
        AppError::Cancelled(_) => crate::plugin::capability::CapabilityError::Cancelled,
        other => crate::plugin::capability::CapabilityError::Failed {
            message: other.to_string(),
        },
    }
}

struct HostQuestionPort<'a>(&'a dyn ToolHost);

#[async_trait]
impl InteractiveQuestionPort for HostQuestionPort<'_> {
    async fn ask(&self, question: UserQuestion) -> Result<UserAnswer> {
        let response = self
            .0
            .interact(InteractionRequest {
                question: question.question,
                header: question.header,
                options: question
                    .options
                    .into_iter()
                    .map(|option| InteractionOption {
                        label: option.label,
                        description: option.description,
                    })
                    .collect(),
                allow_custom: question.allow_custom,
            })
            .await
            .map_err(|error| AppError::Tool(error.to_string()))?;
        Ok(match response {
            InteractionResponse::Selected(index) => UserAnswer::Selected(index),
            InteractionResponse::Custom(value) => UserAnswer::Custom(value),
            InteractionResponse::Cancelled => UserAnswer::Cancelled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::AskUserQuestionTool;
    use crate::tools::registry::ToolRegistry;
    use rig::tool::{Tool, tool_definition};

    fn assert_native_definition<T: Tool>(tool: &T, capabilities: &BTreeMap<CapabilityId, Arc<dyn ToolCapability>>) {
        let definition = tool_definition(tool);
        let descriptor = capabilities[&format!("tool:{}", T::NAME).parse().unwrap()].descriptor();
        assert_eq!(definition.parameters, descriptor.argument_schema, "{} schema", T::NAME);
        assert_eq!(
            definition.description,
            descriptor.description,
            "{} description",
            T::NAME
        );
    }

    #[test]
    fn declarations_match_legacy_names_prompts_descriptions_and_schemas() {
        let root = std::env::temp_dir();
        let config = Config::default();
        let catalog = BuiltinToolCatalog::new(&root, &config).unwrap();
        let capabilities = catalog.into_capabilities();
        assert_eq!(capabilities.len(), ToolRegistry::descriptors().len());
        for declaration in DECLARATIONS {
            let legacy = ToolRegistry::descriptor(declaration.name).unwrap();
            let capability = capabilities
                .get(&format!("tool:{}", declaration.name).parse().unwrap())
                .unwrap();
            let descriptor = capability.descriptor();
            assert_eq!(legacy.prompt, descriptor.prompt_guidance);
            assert_eq!(legacy.description, descriptor.description);
            assert_eq!(legacy.capability, declaration.capability);
        }

        assert_native_definition(&ReadTool::new(&root), &capabilities);
        assert_native_definition(&WriteTool::new(&root), &capabilities);
        assert_native_definition(&EditTool::new(&root), &capabilities);
        assert_native_definition(&BashTool::new(&root), &capabilities);
        assert_native_definition(&AskUserTool::new(), &capabilities);
        assert_native_definition(&AskUserQuestionTool::default(), &capabilities);
        let http = HttpClient::new(false).unwrap();
        assert_native_definition(
            &WebSearchTool::new(
                http.clone(),
                SearchRateLimiter::new(0),
                WebSearchConfig {
                    region: "wt-wt".to_string(),
                    timeout_sec: 1,
                },
            ),
            &capabilities,
        );
        assert_native_definition(
            &WebFetchTool::new(
                http,
                FetchCache::new(60, 4),
                WebFetchConfig {
                    timeout_sec: 1,
                    max_bytes: 1024,
                    default_limit: 20,
                },
            ),
            &capabilities,
        );
    }
}
