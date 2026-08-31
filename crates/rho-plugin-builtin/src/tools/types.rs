use rho_core::error::AppError;
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

pub fn normalize_schema(value: &mut serde_json::Value) {
    let mut defs = std::collections::HashMap::new();
    collect_definitions(value, &mut defs);
    inline_refs(value, &defs);
    clean_schema(value);
    if let serde_json::Value::Object(map) = value {
        map.remove("$defs");
        map.remove("definitions");
        map.remove("$schema");
    }
}

fn collect_definitions(value: &serde_json::Value, defs: &mut std::collections::HashMap<String, serde_json::Value>) {
    if let serde_json::Value::Object(map) = value {
        for key in ["$defs", "definitions"] {
            if let Some(serde_json::Value::Object(submap)) = map.get(key) {
                for (name, def) in submap {
                    defs.insert(format!("#/{key}/{name}"), def.clone());
                    defs.insert(format!("#/$defs/{name}"), def.clone());
                    defs.insert(format!("#/definitions/{name}"), def.clone());
                    defs.insert(name.clone(), def.clone());
                }
            }
        }
        for subval in map.values() {
            collect_definitions(subval, defs);
        }
    } else if let serde_json::Value::Array(arr) = value {
        for item in arr {
            collect_definitions(item, defs);
        }
    }
}

fn inline_refs(value: &mut serde_json::Value, defs: &std::collections::HashMap<String, serde_json::Value>) {
    if let serde_json::Value::Object(map) = value {
        if let Some(serde_json::Value::String(ref_target)) = map.get("$ref")
            && let Some(target_def) = defs.get(ref_target)
        {
            let mut inlined = target_def.clone();
            inline_refs(&mut inlined, defs);
            *value = inlined;
            return;
        }
        for subval in map.values_mut() {
            inline_refs(subval, defs);
        }
    } else if let serde_json::Value::Array(arr) = value {
        for item in arr {
            inline_refs(item, defs);
        }
    }
}

fn clean_schema(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Bool(true) => {
            *value = serde_json::Value::Object(serde_json::Map::new());
        }
        serde_json::Value::Object(map) => {
            if map.get("default") == Some(&serde_json::Value::Null) {
                map.remove("default");
            }
            if let Some(serde_json::Value::Array(arr)) = map.get("type") {
                let non_null: Vec<_> = arr
                    .iter()
                    .filter(|item| item.as_str() != Some("null"))
                    .cloned()
                    .collect();
                if non_null.len() == 1 {
                    map.insert("type".to_string(), non_null[0].clone());
                }
            }
            if let Some(serde_json::Value::Array(arr)) = map.get("anyOf") {
                let non_null: Vec<_> = arr
                    .iter()
                    .filter(|item| {
                        !(item.is_object()
                            && item.as_object().unwrap().get("type")
                                == Some(&serde_json::Value::String("null".to_string())))
                    })
                    .cloned()
                    .collect();
                if non_null.len() == 1 {
                    let mut single = non_null[0].clone();
                    clean_schema(&mut single);
                    *value = single;
                    return;
                }
            }
            for subval in map.values_mut() {
                clean_schema(subval);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                clean_schema(item);
            }
        }
        _ => {}
    }
}

pub fn generated_schema<T: JsonSchema>() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(T)).expect("generated JSON Schema must serialize");
    normalize_schema(&mut schema);
    schema
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
    use super::*;
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
    fn normalize_schema_replaces_boolean_subschemas() {
        let mut schema = serde_json::json!({
            "$defs": {
                "Item": {
                    "type": "string"
                }
            },
            "type": "object",
            "properties": {
                "options": {
                    "type": ["array", "null"],
                    "items": true
                },
                "item": {
                    "$ref": "#/$defs/Item"
                },
                "extra": true
            },
            "prefixItems": [true],
            "anyOf": [true, {"type": "string"}]
        });
        normalize_schema(&mut schema);
        assert_eq!(
            schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "options": {
                        "type": "array",
                        "items": {}
                    },
                    "item": {
                        "type": "string"
                    },
                    "extra": {}
                },
                "prefixItems": [{}],
                "anyOf": [{}, {"type": "string"}]
            })
        );
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
