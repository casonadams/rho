use crate::error::AppError;
use rig::tool::ToolExecutionError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            metadata: None,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            metadata: None,
        }
    }
}

pub fn generated_schema<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(T)).expect("generated JSON Schema must serialize")
}

pub fn into_rig_result(result: Result<ToolResult, AppError>) -> Result<String, ToolExecutionError> {
    match result {
        Ok(result) if result.is_error => Err(ToolExecutionError::other(result.content)),
        Ok(result) => Ok(result.content),
        Err(error) => Err(ToolExecutionError::from_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::web::{
        FetchCache, HttpClient, SearchRateLimiter, WebFetchConfig, WebFetchTool, WebSearchConfig, WebSearchTool,
    };
    use crate::tools::{AskUserTool, BashTool, EditTool, ReadTool, WriteTool};
    use rig::tool::{ToolContext, ToolErrorKind, ToolSet};

    fn tool_set() -> ToolSet {
        let base = std::env::temp_dir();
        let http = HttpClient::new(false).unwrap();
        let mut tools = ToolSet::default();
        tools.add_tool(ReadTool::new(&base));
        tools.add_tool(WriteTool::new(&base));
        tools.add_tool(EditTool::new(&base));
        tools.add_tool(BashTool::new(&base));
        tools.add_tool(AskUserTool::new());
        tools.add_tool(WebSearchTool::new(
            http.clone(),
            SearchRateLimiter::new(0),
            WebSearchConfig {
                region: "wt-wt".to_string(),
                timeout_sec: 1,
            },
        ));
        tools.add_tool(WebFetchTool::new(
            http,
            FetchCache::new(60, 4),
            WebFetchConfig {
                timeout_sec: 1,
                max_bytes: 1024,
                default_limit: 20,
            },
        ));
        tools
    }

    #[test]
    fn rig_schemas_are_generated_from_typed_arguments() {
        let tools = tool_set();
        let expected = [
            ("read", &["path"][..]),
            ("write", &["content", "path"][..]),
            ("edit", &["edits", "path"][..]),
            ("bash", &["command"][..]),
            ("websearch", &["query"][..]),
            ("webfetch", &["url"][..]),
        ];

        for (name, required) in expected {
            let definition = tools
                .get_tool_definitions()
                .into_iter()
                .find(|definition| definition.name == name)
                .unwrap();
            let schema_required = definition.parameters["required"].as_array().unwrap();
            for field in required {
                assert!(schema_required.iter().any(|value| value == field), "{name}.{field}");
            }
        }
    }

    #[tokio::test]
    async fn rig_dispatch_rejects_malformed_arguments_for_every_tool() {
        let tools = tool_set();
        for name in ["read", "write", "edit", "bash", "websearch", "webfetch"] {
            let result = tools
                .execute(name, "{\"unexpected\":true}", &mut ToolContext::new())
                .await;
            assert!(result.is_error_kind(ToolErrorKind::InvalidArgs), "{name}: {result:?}");
        }
    }

    #[tokio::test]
    async fn rig_dispatch_rejects_unknown_tools() {
        let result = tool_set().execute("unknown", "{}", &mut ToolContext::new()).await;
        assert!(result.is_error_kind(ToolErrorKind::NotFound));
    }
}
