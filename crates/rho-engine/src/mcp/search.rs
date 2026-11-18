use super::client::McpToolDefinition;
use crate::tools::{EngineTool, GenericTool, ToolResult};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolEntry {
    pub server_name: String,
    pub name: String,
    pub wire_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub keywords: Vec<String>,
    pub direct: bool,
}

impl McpToolEntry {
    pub fn new(server: &str, tool: McpToolDefinition, search_keywords: &BTreeMap<String, Vec<String>>) -> Self {
        let keywords = ["*", tool.name.as_str()]
            .iter()
            .filter_map(|key| search_keywords.get(*key))
            .flatten()
            .cloned()
            .collect();
        Self {
            server_name: server.to_string(),
            wire_name: super::naming::wire_name(server, &tool.name),
            name: tool.name,
            description: tool.description.unwrap_or_default(),
            input_schema: tool.input_schema,
            keywords,
            direct: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    Connecting,
    Unavailable(String),
    Stale(String),
}

#[derive(Default)]
struct CatalogState {
    tools: Vec<McpToolEntry>,
    servers: BTreeMap<String, ServerState>,
}

/// Every discovered MCP tool. The model reaches non-direct entries through `tool_search` and the `mcp` gateway,
/// so later discoveries extend the catalog without changing the provider tool list.
#[derive(Clone, Default)]
pub struct ToolSearchCatalog {
    state: Arc<RwLock<CatalogState>>,
}

impl ToolSearchCatalog {
    pub fn new(tools: Vec<McpToolEntry>) -> Self {
        let catalog = Self::default();
        catalog.state.write().unwrap().tools = tools;
        catalog
    }

    pub fn is_empty(&self) -> bool {
        self.state.read().unwrap().tools.is_empty()
    }

    pub fn deferred_count(&self) -> usize {
        self.state.read().unwrap().tools.iter().filter(|t| !t.direct).count()
    }

    pub fn replace_server_tools(&self, server: &str, entries: Vec<McpToolEntry>) {
        let mut state = self.state.write().unwrap();
        let direct: HashSet<String> = state
            .tools
            .iter()
            .filter(|t| t.server_name == server && t.direct)
            .map(|t| t.name.clone())
            .collect();
        state.tools.retain(|t| t.server_name != server);
        state.tools.extend(entries.into_iter().map(|mut entry| {
            entry.direct |= direct.contains(&entry.name);
            entry
        }));
        state.servers.remove(server);
    }

    pub fn set_server_state(&self, server: &str, server_state: ServerState) {
        self.state
            .write()
            .unwrap()
            .servers
            .insert(server.to_string(), server_state);
    }

    pub fn server_states(&self) -> BTreeMap<String, ServerState> {
        self.state.read().unwrap().servers.clone()
    }

    pub fn find(&self, server: Option<&str>, tool: &str) -> Option<McpToolEntry> {
        let state = self.state.read().unwrap();
        let found = match server {
            Some(server) => state
                .tools
                .iter()
                .find(|t| t.server_name == server && (t.name == tool || t.wire_name == tool)),
            None => state.tools.iter().find(|t| t.wire_name == tool),
        };
        found.cloned()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<McpToolEntry> {
        let q = query.trim().to_lowercase();
        let tokens: Vec<&str> = q
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty() {
            return Vec::new();
        }

        let state = self.state.read().unwrap();
        let mut scored: Vec<(usize, &McpToolEntry)> = state
            .tools
            .iter()
            .filter(|t| !t.direct)
            .map(|t| (score_tool_match(t, &q, &tokens), t))
            .filter(|(score, _)| *score > 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.wire_name.cmp(&b.1.wire_name)));
        scored.into_iter().take(limit.max(1)).map(|(_, t)| t.clone()).collect()
    }

    pub fn catalog_summary(&self) -> String {
        let state = self.state.read().unwrap();
        let mut by_server: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for t in state.tools.iter().filter(|t| !t.direct) {
            by_server.entry(&t.server_name).or_default().push(&t.name);
        }
        let mut parts: Vec<String> = by_server
            .into_iter()
            .map(|(server, names)| format!("{server}: {}", names.join(", ")))
            .collect();
        parts.extend(
            state
                .servers
                .iter()
                .filter(|(_, s)| **s == ServerState::Connecting)
                .map(|(server, _)| format!("{server}: (connecting)")),
        );
        parts.join("; ")
    }

    fn server_state_note(&self) -> Option<String> {
        let states = self.server_states();
        if states.is_empty() {
            return None;
        }
        let lines: Vec<String> = states
            .iter()
            .map(|(server, state)| match state {
                ServerState::Connecting => format!("- {server}: still connecting; search again shortly"),
                ServerState::Unavailable(msg) => format!("- {server}: unavailable: {msg}"),
                ServerState::Stale(msg) => format!("- {server}: tool list may be outdated: {msg}"),
            })
            .collect();
        Some(format!("MCP server status:\n{}", lines.join("\n")))
    }

    pub fn format_search_results(&self, query: &str, matches: &[McpToolEntry]) -> String {
        let mut out = if matches.is_empty() {
            let state = self.state.read().unwrap();
            let names: Vec<String> = state
                .tools
                .iter()
                .filter(|t| !t.direct)
                .map(|t| format!("{}/{}", t.server_name, t.name))
                .collect();
            format!(
                "No MCP tools matched query '{query}'. Available deferred tools: {}.",
                names.join(", ")
            )
        } else {
            let mut out = format!(
                "Found {} tool(s). Call one with the `mcp` tool: {{\"server\": <server>, \"tool\": <tool>, \"arguments\": {{...}}}}.",
                matches.len()
            );
            for m in matches {
                let schema = serde_json::to_string(&m.input_schema).unwrap_or_default();
                out.push_str(&format!(
                    "\n- server `{}`, tool `{}`: {} (parameters: {})\n  input schema: {schema}",
                    m.server_name,
                    m.name,
                    m.description,
                    format_parameter_summary(&m.input_schema)
                ));
            }
            out
        };
        if let Some(note) = self.server_state_note() {
            out.push_str("\n\n");
            out.push_str(&note);
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

fn score_tool_match(tool: &McpToolEntry, query: &str, tokens: &[&str]) -> usize {
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

pub fn build_tool_search_tool(catalog: ToolSearchCatalog) -> Arc<dyn EngineTool> {
    let summary = catalog.catalog_summary();
    let description = format!(
        "Search the deferred MCP tool catalog. Matches include each tool's input schema; call them with the `mcp` tool. \
         For external services, databases, or APIs, always search here for specialized MCP tools instead of using CLI commands in bash. \
         Keywords match tool names, descriptions, and parameters; exact matches win. Deferred tools: {summary}"
    );

    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Keywords or an exact tool name to search for"
            },
            "limit": {
                "type": "integer",
                "description": "Maximum number of matching tools to return (default: 5)"
            }
        },
        "required": ["query"]
    });

    Arc::new(GenericTool::new("tool_search", description, schema, move |args| {
        let catalog = catalog.clone();
        async move {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("").trim();
            let limit = args
                .get("limit")
                .and_then(|l| l.as_u64())
                .map_or(5, |l| usize::try_from(l).unwrap_or(usize::MAX));
            let matches = catalog.search(query, limit);
            Ok(ToolResult::success(catalog.format_search_results(query, &matches)))
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(server: &str, name: &str, description: &str, schema: serde_json::Value) -> McpToolEntry {
        McpToolEntry::new(
            server,
            McpToolDefinition {
                name: name.to_string(),
                description: Some(description.to_string()),
                input_schema: schema,
            },
            &BTreeMap::new(),
        )
    }

    fn sample_tools() -> Vec<McpToolEntry> {
        vec![
            entry(
                "github",
                "create_issue",
                "Create an issue in a repository",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "repo": { "type": "string" },
                        "title": { "type": "string" },
                        "body": { "type": "string" }
                    },
                    "required": ["repo", "title"]
                }),
            ),
            entry(
                "github",
                "list_issues",
                "List issues for a repository",
                serde_json::json!({
                    "type": "object",
                    "properties": { "repo": { "type": "string" }, "state": { "type": "string" } },
                    "required": ["repo"]
                }),
            ),
            entry(
                "weather",
                "get_forecast",
                "Get forecast and temperature predictions",
                serde_json::json!({
                    "type": "object",
                    "properties": { "city": { "type": "string" } },
                    "required": ["city"]
                }),
            ),
        ]
    }

    #[test]
    fn test_tool_search_exact_name_match() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        assert_eq!(catalog.search("create_issue", 5)[0].wire_name, "github_create_issue");
        assert_eq!(
            catalog.search("github_create_issue", 5)[0].wire_name,
            "github_create_issue"
        );
    }

    #[test]
    fn test_tool_search_keyword_and_description_match() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        let results = catalog.search("temperature", 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].wire_name, "weather_get_forecast");
        assert_eq!(catalog.search("issues", 5).len(), 2);
    }

    #[test]
    fn test_tool_search_configured_keywords_match() {
        let keywords = BTreeMap::from([("create_issue".to_string(), vec!["ticket".to_string()])]);
        let tool = McpToolEntry::new(
            "github",
            McpToolDefinition {
                name: "create_issue".into(),
                description: None,
                input_schema: serde_json::json!({}),
            },
            &keywords,
        );
        let catalog = ToolSearchCatalog::new(vec![tool]);
        assert_eq!(catalog.search("ticket", 5)[0].wire_name, "github_create_issue");
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
        assert!(formatted.contains("github/create_issue"));
    }

    #[test]
    fn search_results_carry_schema_and_gateway_call_shape() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        let results = catalog.search("forecast", 5);
        let formatted = catalog.format_search_results("forecast", &results);
        assert!(formatted.contains("Found 1 tool(s)"));
        assert!(formatted.contains("`mcp` tool"));
        assert!(formatted.contains("server `weather`, tool `get_forecast`"));
        assert!(formatted.contains("city (required)"));
        assert!(formatted.contains(r#"input schema: {"properties":{"city":{"type":"string"}}"#));
    }

    #[test]
    fn direct_tools_are_findable_but_not_searched() {
        let mut tools = sample_tools();
        tools[2].direct = true;
        let catalog = ToolSearchCatalog::new(tools);
        assert!(catalog.search("forecast", 5).is_empty());
        assert_eq!(catalog.deferred_count(), 2);
        assert!(catalog.find(Some("weather"), "get_forecast").is_some());
        assert!(catalog.find(None, "weather_get_forecast").is_some());
        assert!(catalog.find(Some("github"), "get_forecast").is_none());
        assert!(!catalog.catalog_summary().contains("get_forecast"));
    }

    #[test]
    fn replacing_server_tools_keeps_direct_flags_and_clears_state() {
        let mut tools = sample_tools();
        tools[0].direct = true;
        let catalog = ToolSearchCatalog::new(tools);
        catalog.set_server_state("github", ServerState::Stale("offline".into()));
        catalog.replace_server_tools(
            "github",
            vec![
                entry("github", "create_issue", "new text", serde_json::json!({})),
                entry("github", "close_issue", "Close an issue", serde_json::json!({})),
            ],
        );
        assert!(catalog.find(Some("github"), "create_issue").unwrap().direct);
        assert!(!catalog.find(Some("github"), "close_issue").unwrap().direct);
        assert!(catalog.find(Some("github"), "list_issues").is_none());
        assert!(catalog.server_states().is_empty());
    }

    #[test]
    fn server_states_are_reported_with_search_results() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        catalog.set_server_state("slow", ServerState::Connecting);
        catalog.set_server_state("broken", ServerState::Unavailable("spawn failed".into()));
        assert!(catalog.catalog_summary().contains("slow: (connecting)"));
        let formatted = catalog.format_search_results("zzz", &[]);
        assert!(formatted.contains("- broken: unavailable: spawn failed"));
        assert!(formatted.contains("- slow: still connecting"));
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
    async fn tool_search_does_not_change_the_tool_list() {
        let catalog = ToolSearchCatalog::new(sample_tools());
        let tool = build_tool_search_tool(catalog.clone());
        let result = tool.execute(serde_json::json!({"query": "forecast"})).await.unwrap();
        assert!(result.content.contains("get_forecast"));
        assert_eq!(catalog.deferred_count(), 3);
    }
}
