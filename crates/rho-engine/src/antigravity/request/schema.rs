use rig::completion::{CompletionRequest, ToolDefinition};
use rig::message::ToolChoice;
use serde_json::{Value, json};

pub fn strip_meta_schema(schema: &Value) -> Value {
    const META_KEYS: [&str; 9] = [
        "$schema",
        "$id",
        "$anchor",
        "$dynamicAnchor",
        "$vocabulary",
        "$comment",
        "$defs",
        "definitions",
        "additionalProperties",
    ];
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(strip_meta_schema).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !META_KEYS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), strip_meta_schema(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Cloud Code Assist's Claude/GPT-OSS custom-tool bridge accepts only a
/// protobuf `Schema` subset; anything else 400s with `Unknown name`.
fn normalize_type_value(value: &Value) -> Option<Value> {
    match value {
        Value::String(s) => Some(json!(s)),
        Value::Array(items) => items
            .iter()
            .find(|v| v.is_string() && v.as_str() != Some("null"))
            .cloned(),
        _ => None,
    }
}

fn insert_normalized_field(out: &mut serde_json::Map<String, Value>, (key, value): (&str, &Value), allowed: &[&str]) {
    if key == "type" {
        if let Some(t) = normalize_type_value(value) {
            out.insert("type".into(), t);
        }
    } else if (key == "properties" && value.is_object()) || allowed.contains(&key) {
        out.insert(key.to_string(), normalize_custom_tool_schema(value));
    }
}

pub fn normalize_custom_tool_schema(schema: &Value) -> Value {
    const ALLOWED: [&str; 5] = ["type", "description", "properties", "required", "items"];
    match schema {
        Value::Array(items) => Value::Array(items.iter().map(normalize_custom_tool_schema).collect()),
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                insert_normalized_field(&mut out, (key, value), &ALLOWED);
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

pub fn ensure_root_object(schema: &Value) -> Value {
    match schema {
        Value::Object(map) if map.contains_key("type") => schema.clone(),
        Value::Object(map) => {
            let mut out = map.clone();
            out.insert("type".to_string(), json!("object"));
            out.entry("properties").or_insert_with(|| json!({}));
            Value::Object(out)
        }
        _ => json!({ "type": "object", "properties": {} }),
    }
}

pub fn convert_tools(request: &CompletionRequest, legacy_parameters: bool) -> Option<Value> {
    if request.tools.is_empty() {
        return None;
    }
    let declarations: Vec<Value> = request
        .tools
        .iter()
        .map(|tool: &ToolDefinition| {
            let schema = strip_meta_schema(&tool.parameters);
            let schema = ensure_root_object(&schema);
            let mut declaration = json!({
                "name": tool.name,
                "description": tool.description,
            });
            if legacy_parameters {
                declaration["parameters"] = normalize_custom_tool_schema(&schema);
            } else {
                declaration["parametersJsonSchema"] = schema;
            }
            declaration
        })
        .collect();
    Some(json!([{ "functionDeclarations": declarations }]))
}

pub fn tool_config_mode(choice: Option<ToolChoice>) -> &'static str {
    match choice {
        Some(ToolChoice::None) => "NONE",
        Some(ToolChoice::Required | ToolChoice::Specific { .. }) => "ANY",
        _ => "VALIDATED",
    }
}
