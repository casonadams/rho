use super::client::{McpClient, McpToolDefinition};
use super::pool::McpServerPool;
use rig::tool::{DynamicTool, ToolOutput};
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub type McpCallFuture = Pin<Box<dyn Future<Output = String> + Send>>;
pub type McpCallFn = Arc<dyn Fn(String, serde_json::Value) -> McpCallFuture + Send + Sync>;

#[derive(Clone)]
pub struct DeferredMcpTool {
    pub server_name: String,
    pub name: String,
    pub wire_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    call_fn: McpCallFn,
    pub keywords: Vec<String>,
}

impl DeferredMcpTool {
    pub fn new(
        server_name: String,
        name: String,
        description: String,
        input_schema: serde_json::Value,
        client: Arc<McpClient>,
        max_bytes: usize,
    ) -> Self {
        let wire_name = format!("{server_name}_{name}");
        let call_fn = Arc::new(move |tool_name: String, args: serde_json::Value| {
            let client = Arc::clone(&client);
            Box::pin(async move {
                match client.call_tool(&tool_name, args).await {
                    Ok(result) => {
                        let text = result.as_text_truncated(max_bytes);
                        if result.is_error.unwrap_or(false) {
                            format!("[Error] {text}")
                        } else {
                            text
                        }
                    }
                    Err(e) => format!("[MCP Error] {e}"),
                }
            }) as McpCallFuture
        });
        Self {
            server_name,
            name,
            wire_name,
            description,
            input_schema,
            call_fn,
            keywords: Vec::new(),
        }
    }

    pub fn from_definition(
        server_name: String,
        tool: McpToolDefinition,
        pool: McpServerPool,
        max_bytes: usize,
        keywords: Vec<String>,
    ) -> Self {
        let wire_name = format!("{server_name}_{}", tool.name);
        let s_name = server_name.clone();
        let call_fn = Arc::new(move |tool_name: String, args: serde_json::Value| {
            let pool = pool.clone();
            let server_name = s_name.clone();
            Box::pin(async move {
                match pool.get_or_connect(&server_name).await {
                    Ok((client, in_flight)) => {
                        let res = client.call_tool(&tool_name, args).await;
                        in_flight.fetch_sub(1, Ordering::SeqCst);
                        pool.touch_activity(&server_name).await;
                        match res {
                            Ok(result) => {
                                let text = result.as_text_truncated(max_bytes);
                                if result.is_error.unwrap_or(false) {
                                    format!("[Error] {text}")
                                } else {
                                    text
                                }
                            }
                            Err(e) => format!("[MCP Error] {e}"),
                        }
                    }
                    Err(e) => format!("[MCP Error] Failed to connect to server '{server_name}': {e}"),
                }
            }) as McpCallFuture
        });
        Self {
            server_name,
            name: tool.name,
            wire_name,
            description: tool.description.unwrap_or_default(),
            input_schema: tool.input_schema,
            call_fn,
            keywords,
        }
    }

    pub async fn execute_call(&self, args: serde_json::Value) -> String {
        (self.call_fn)(self.name.clone(), args).await
    }

    pub fn into_dynamic_tool(&self) -> DynamicTool {
        let tool_name = self.wire_name.clone();
        let description = self.description.clone();
        let mut schema = self.input_schema.clone();
        crate::tools::normalize_schema(&mut schema);
        let tool = self.clone();

        let tn =
            rig::message::ToolName::new(&tool_name).unwrap_or_else(|_| rig::message::ToolName::new("unknown").unwrap());
        DynamicTool::new(tn, description, schema, move |args| {
            let tool = tool.clone();
            Box::pin(async move {
                let output = tool.execute_call(args).await;
                Ok(ToolOutput::text(output))
            })
        })
    }
}

#[derive(Clone, Default)]
pub struct ToolSearchCatalog {
    tools: Arc<std::sync::RwLock<Vec<DeferredMcpTool>>>,
}

impl ToolSearchCatalog {
    pub fn new(tools: Vec<DeferredMcpTool>) -> Self {
        Self {
            tools: Arc::new(std::sync::RwLock::new(tools)),
        }
    }

    pub fn len(&self) -> usize {
        self.tools.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.read().unwrap().is_empty()
    }

    pub fn add_tool(&self, tool: DeferredMcpTool) {
        self.tools.write().unwrap().push(tool);
    }

    pub fn list_tools(&self) -> Vec<DeferredMcpTool> {
        self.tools.read().unwrap().clone()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<DeferredMcpTool> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let tokens: Vec<&str> = q
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty() {
            return Vec::new();
        }

        let tools = self.tools.read().unwrap();
        let mut scored: Vec<(usize, &DeferredMcpTool)> = tools
            .iter()
            .filter_map(|t| {
                let score = score_tool_match(t, &q, &tokens);
                if score > 0 { Some((score, t)) } else { None }
            })
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.wire_name.cmp(&b.1.wire_name)));
        scored.into_iter().take(limit.max(1)).map(|(_, t)| t.clone()).collect()
    }

    pub fn catalog_summary(&self) -> String {
        let tools = self.tools.read().unwrap();
        let mut by_server: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for t in tools.iter() {
            by_server.entry(&t.server_name).or_default().push(&t.name);
        }
        let mut lines = Vec::new();
        for (server, names) in by_server {
            lines.push(format!("{server}: {}", names.join(", ")));
        }
        lines.join("; ")
    }

    pub fn format_search_results(&self, query: &str, matches: &[DeferredMcpTool]) -> String {
        if matches.is_empty() {
            let tools = self.tools.read().unwrap();
            let names: Vec<&str> = tools.iter().map(|t| t.wire_name.as_str()).collect();
            return format!(
                "No MCP tools matched query '{query}'. Available deferred tools in catalog: {}.",
                names.join(", ")
            );
        }

        let mut out = format!(
            "Loaded {} tool(s) into active tools for subsequent turns:",
            matches.len()
        );
        for m in matches {
            let params = format_parameter_summary(&m.input_schema);
            out.push_str(&format!(
                "\n- `{}`: {} (parameters: {})",
                m.wire_name, m.description, params
            ));
        }
        out
    }
}

fn format_parameter_summary(schema: &serde_json::Value) -> String {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return "none".to_string();
    };
    if props.is_empty() {
        return "none".to_string();
    }
    let required: HashSet<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut parts = Vec::new();
    for key in props.keys() {
        if required.contains(key.as_str()) {
            parts.push(format!("{key} (required)"));
        } else {
            parts.push(key.clone());
        }
    }
    parts.join(", ")
}

struct ToolMatchTarget<'a> {
    wire: String,
    name: String,
    server: String,
    desc: String,
    keywords: &'a [String],
    props: Option<&'a serde_json::Map<String, serde_json::Value>>,
}

fn score_property_tokens(props: Option<&serde_json::Map<String, serde_json::Value>>, token: &str, stem: &str) -> usize {
    let Some(props) = props else { return 0 };
    let mut score = 0;
    for prop in props.keys() {
        let prop_lower = prop.to_lowercase();
        if prop_lower.contains(token) || prop_lower.contains(stem) {
            score += 20;
        }
    }
    score
}

fn score_keyword_tokens(keywords: &[String], token: &str, stem: &str) -> usize {
    let mut score = 0;
    for kw in keywords {
        let kw_lower = kw.to_lowercase();
        if kw_lower == token {
            score += 600;
        } else if kw_lower.contains(token) || (!stem.is_empty() && kw_lower.contains(stem)) {
            score += 150;
        }
    }
    score
}

fn score_token(token: &str, target: &ToolMatchTarget<'_>) -> usize {
    let stem = token.strip_suffix('s').unwrap_or(token);
    let mut score = 0;
    if target.wire == token || target.name == token {
        score += 500;
    } else if target.wire.contains(token) || target.name.contains(token) {
        score += 100;
    } else if !stem.is_empty() && (target.wire.contains(stem) || target.name.contains(stem)) {
        score += 80;
    }

    score += score_keyword_tokens(target.keywords, token, stem);

    if target.server.contains(token) || target.server.contains(stem) {
        score += 50;
    }

    if target.desc.contains(token) || target.desc.contains(stem) {
        score += 30;
    }

    score + score_property_tokens(target.props, token, stem)
}

fn score_tool_match(tool: &DeferredMcpTool, query: &str, tokens: &[&str]) -> usize {
    let target = ToolMatchTarget {
        wire: tool.wire_name.to_lowercase(),
        name: tool.name.to_lowercase(),
        server: tool.server_name.to_lowercase(),
        desc: tool.description.to_lowercase(),
        keywords: &tool.keywords,
        props: tool.input_schema.get("properties").and_then(|p| p.as_object()),
    };

    let mut score = if target.wire == query || target.name == query {
        1000
    } else {
        0
    };
    for &token in tokens {
        score += score_token(token, &target);
    }
    score
}

type SharedToolNames = Arc<std::sync::RwLock<Vec<String>>>;

#[derive(Clone, Default)]
pub struct DynamicToolActivator {
    server_handle: Arc<std::sync::RwLock<Option<rig::tool::server::ToolServerHandle>>>,
    tool_names: Arc<std::sync::RwLock<Option<SharedToolNames>>>,
    activated_names: Arc<std::sync::RwLock<HashSet<String>>>,
}

impl DynamicToolActivator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn attach(&self, handle: rig::tool::server::ToolServerHandle, tool_names: SharedToolNames) {
        *self.server_handle.write().unwrap() = Some(handle);
        *self.tool_names.write().unwrap() = Some(tool_names);
    }

    pub fn mark_activated(&self, name: &str) {
        self.activated_names.write().unwrap().insert(name.to_string());
    }

    pub fn is_activated(&self, name: &str) -> bool {
        self.activated_names.read().unwrap().contains(name)
    }

    pub async fn activate_tools(&self, tools: &[DeferredMcpTool]) {
        let mut to_activate = Vec::new();
        {
            let mut activated = self.activated_names.write().unwrap();
            for tool in tools {
                if activated.insert(tool.wire_name.clone()) {
                    to_activate.push(tool.clone());
                }
            }
        }
        let handle_opt = self.server_handle.read().unwrap().clone();
        let tool_names_opt = self.tool_names.read().unwrap().clone();

        for tool in to_activate {
            if let Some(handle) = handle_opt.as_ref() {
                handle.add_dynamic_tool(tool.into_dynamic_tool());
            }
            if let Some(names_ref) = tool_names_opt.as_ref() {
                names_ref.write().unwrap().push(tool.wire_name.clone());
            }
        }
    }
}

pub fn extract_invoked_tool_names(messages: &[rho_harness_core::model::ChatMessage]) -> HashSet<String> {
    let mut names = HashSet::new();
    for msg in messages {
        if let rho_harness_core::model::ChatMessage::Assistant { content, .. } = msg {
            for item in content {
                if let rho_harness_core::model::AssistantContent::ToolCall(call) = item {
                    names.insert(call.function.name.clone());
                }
            }
        }
    }
    names
}

fn resolve_executable_tool<'a>(
    matches: &'a [DeferredMcpTool],
    target_tool: Option<&str>,
    query: &str,
) -> Result<&'a DeferredMcpTool, String> {
    if matches.is_empty() {
        return Err("No tools matched the query to execute.".to_string());
    }

    if let Some(target) = target_tool {
        let trimmed = target.trim();
        if !trimmed.is_empty() {
            return matches
                .iter()
                .find(|m| m.wire_name == trimmed || m.name == trimmed)
                .ok_or_else(|| {
                    let available: Vec<&str> = matches.iter().map(|m| m.wire_name.as_str()).collect();
                    format!(
                        "Target tool '{trimmed}' not found among search matches: [{}].",
                        available.join(", ")
                    )
                });
        }
    }

    if matches.len() == 1 {
        return Ok(&matches[0]);
    }

    let q_trimmed = query.trim();
    if let Some(exact) = matches.iter().find(|m| m.name == q_trimmed || m.wire_name == q_trimmed) {
        return Ok(exact);
    }

    let available: Vec<&str> = matches.iter().map(|m| m.wire_name.as_str()).collect();
    Err(format!(
        "Multiple tools matched; specify 'tool' in 'execute'. Candidates: [{}].",
        available.join(", ")
    ))
}

async fn maybe_execute_tool(
    matches: &[DeferredMcpTool],
    exec_val: Option<&serde_json::Map<String, serde_json::Value>>,
    query: &str,
) -> Option<String> {
    let exec = exec_val?;
    let target_tool = exec.get("tool").and_then(|t| t.as_str());
    let call_args = exec.get("arguments").cloned().unwrap_or(serde_json::json!({}));
    match resolve_executable_tool(matches, target_tool, query) {
        Ok(tool) => {
            let res = tool.execute_call(call_args).await;
            Some(format!("\n\nExecuted `{}`:\n{res}", tool.wire_name))
        }
        Err(err) => Some(format!("\n\n[Execution Error] {err}")),
    }
}

pub fn build_tool_search_tool(catalog: ToolSearchCatalog, activator: DynamicToolActivator) -> DynamicTool {
    let summary = catalog.catalog_summary();
    let description = format!(
        "Search and dynamically load deferred MCP tools into active tools for subsequent turns. \
         Some tools are not loaded upfront; for external services, databases, or APIs, always use 'tool_search' \
         to discover specialized MCP tools instead of CLI commands in bash. \
         Keywords match tool names, descriptions, parameters; exact matches win. \
         To execute immediately, provide 'execute'. Deferred tools: {summary}"
    );

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Keywords or an exact tool name to search for and activate"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of matching tools to load (default: 5)"
            },
            "execute": {
                "type": "object",
                "properties": {
                    "tool": {
                        "type": "string",
                        "description": "Tool name or wire name to execute immediately (optional if only one tool matches or query is exact)"
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments to pass to the tool call"
                    }
                },
                "description": "Optional: immediately execute a matching tool in the same turn"
            }
        },
        "required": ["query"]
    });

    let tn = rig::message::ToolName::new("tool_search").unwrap();
    DynamicTool::new(tn, description, schema, move |args| {
        let catalog = catalog.clone();
        let activator = activator.clone();
        Box::pin(async move {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("").trim();
            let limit = args
                .get("limit")
                .and_then(|l| l.as_u64())
                .map(|l| l as usize)
                .unwrap_or(5);
            let matches = catalog.search(query, limit);
            activator.activate_tools(&matches).await;
            let mut output = catalog.format_search_results(query, &matches);
            if let Some(exec_result) =
                maybe_execute_tool(&matches, args.get("execute").and_then(|e| e.as_object()), query).await
            {
                output.push_str(&exec_result);
            }
            Ok(ToolOutput::text(output))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::transport::McpTransport;

    fn make_test_client(name: &str) -> Arc<McpClient> {
        let transport = McpTransport::new_http(
            "http://127.0.0.1:9999",
            rho_harness_core::config::McpTransportKind::StreamableHttp,
            std::collections::BTreeMap::new(),
            None,
        );
        Arc::new(McpClient::new(name, transport))
    }

    fn sample_tools() -> Vec<DeferredMcpTool> {
        let client_gh = make_test_client("github");
        let client_weather = make_test_client("weather");

        vec![
            DeferredMcpTool::new(
                "github".to_string(),
                "create_issue".to_string(),
                "[MCP: github] Create an issue in a repository".to_string(),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string" },
                        "title": { "type": "string" },
                        "body": { "type": "string" }
                    },
                    "required": ["repo", "title"]
                }),
                client_gh.clone(),
                1000,
            ),
            DeferredMcpTool::new(
                "github".to_string(),
                "list_issues".to_string(),
                "[MCP: github] List issues for a repository".to_string(),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string" },
                        "state": { "type": "string" }
                    },
                    "required": ["repo"]
                }),
                client_gh,
                1000,
            ),
            DeferredMcpTool::new(
                "weather".to_string(),
                "get_forecast".to_string(),
                "[MCP: weather] Get forecast and temperature predictions".to_string(),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }),
                client_weather,
                1000,
            ),
        ]
    }

    #[test]
    fn test_tool_search_exact_name_match() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        let results = catalog.search("create_issue", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].wire_name, "github_create_issue");

        let wire_results = catalog.search("github_create_issue", 5);
        assert!(!wire_results.is_empty());
        assert_eq!(wire_results[0].wire_name, "github_create_issue");
    }

    #[test]
    fn test_tool_search_keyword_and_description_match() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        let results = catalog.search("temperature", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].wire_name, "weather_get_forecast");

        let gh_results = catalog.search("issues", 5);
        assert_eq!(gh_results.len(), 2);
    }

    #[test]
    fn test_tool_search_configured_keywords_match() {
        let mut tools = sample_tools();
        tools[0].keywords = vec!["ticket".to_string(), "bug".to_string()];
        let catalog = ToolSearchCatalog::new(tools);

        let results = catalog.search("ticket", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].wire_name, "github_create_issue");
    }

    #[test]
    fn test_tool_search_parameter_match() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        let results = catalog.search("city", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].wire_name, "weather_get_forecast");
    }

    #[test]
    fn test_tool_search_empty_or_no_match() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        assert!(catalog.search("", 5).is_empty());
        assert!(catalog.search("   ", 5).is_empty());

        let results = catalog.search("nonexistent_widget", 5);
        assert!(results.is_empty());
        let formatted = catalog.format_search_results("nonexistent_widget", &results);
        assert!(formatted.contains("No MCP tools matched query"));
        assert!(formatted.contains("github_create_issue"));
    }

    #[test]
    fn test_tool_search_formatting() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        let results = catalog.search("forecast", 5);
        let formatted = catalog.format_search_results("forecast", &results);
        assert!(formatted.contains("Loaded 1 tool(s)"));
        assert!(formatted.contains("weather_get_forecast"));
        assert!(formatted.contains("city (required)"));
    }

    #[test]
    fn test_extract_invoked_tool_names() {
        let messages = vec![
            rho_harness_core::model::ChatMessage::user("Hello"),
            rho_harness_core::model::ChatMessage::Assistant {
                content: vec![rho_harness_core::model::AssistantContent::ToolCall(
                    rho_harness_core::model::ToolCall::new(
                        "c1",
                        rho_harness_core::model::ToolFunction::new("github_create_issue", serde_json::json!({})),
                    ),
                )],
                id: None,
            },
        ];
        let names = extract_invoked_tool_names(&messages);
        assert_eq!(names.len(), 1);
        assert!(names.contains("github_create_issue"));
    }

    #[tokio::test]
    async fn test_tool_search_dynamic_tool_execution() {
        let handle = rig::tool::server::ToolServer::new().run();
        let activator = DynamicToolActivator::new();
        activator.attach(handle.clone(), Arc::new(std::sync::RwLock::new(Vec::new())));

        let catalog = ToolSearchCatalog::new(sample_tools());
        let tool = build_tool_search_tool(catalog, activator.clone());
        handle.add_dynamic_tool(tool);

        let mut context = rig::tool::ToolContext::default();
        let result = handle
            .execute("tool_search", r#"{"query": "forecast"}"#, &mut context)
            .await;
        let text = result.output().as_text().unwrap_or_default();
        assert!(text.contains("weather_get_forecast"));
        assert!(activator.is_activated("weather_get_forecast"));

        let err_result = handle
            .execute(
                "tool_search",
                r#"{"query": "forecast", "execute": {"tool": "nonexistent"}}"#,
                &mut context,
            )
            .await;
        let err_text = err_result.output().as_text().unwrap_or_default();
        assert!(err_text.contains("[Execution Error] Target tool 'nonexistent' not found"));
    }
}
