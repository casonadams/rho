use super::ToolResult;
use rho_harness_core::text::truncate_chars;
use rho_harness_core::workspace::Workspace;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::PathBuf;

const MAX_ERROR_CHARS: usize = 300;

/// Resolves a model-supplied path inside the workspace; an empty path is reported as a tool error.
pub fn resolve_tool_path(workspace: &Workspace, clean_path: &str, tool: &str) -> Result<PathBuf, ToolResult> {
    workspace
        .resolve(clean_path)
        .ok_or_else(|| ToolResult::error(format!("Empty file path provided for {tool} tool")))
}

fn decode_json_text(value: &Value) -> Option<Value> {
    let text = value.as_str()?.trim();
    if !(text.starts_with('{') || text.starts_with('[')) {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// Decodes arguments sent as one JSON-encoded object so hooks, permission checks, and the tool all see the same value.
pub fn normalize_tool_arguments(args: Value) -> Value {
    match decode_json_text(&args) {
        Some(decoded @ Value::Object(_)) => decoded,
        _ => args,
    }
}

/// Models sometimes send nested arrays or objects as JSON-encoded strings; decode those before giving up.
fn coerce_json_strings(args: &Value) -> Option<Value> {
    let mut map = args.as_object()?.clone();
    let mut changed = false;
    for value in map.values_mut() {
        if let Some(decoded) = decode_json_text(value) {
            *value = decoded;
            changed = true;
        }
    }
    changed.then_some(Value::Object(map))
}

fn expected_shape(schema: &Value) -> String {
    let required: Vec<&str> = schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let optional: Vec<&str> = schema["properties"]
        .as_object()
        .into_iter()
        .flat_map(|props| props.keys())
        .map(String::as_str)
        .filter(|key| !required.contains(key))
        .collect();
    format!(
        "required: {}; optional: {}",
        if required.is_empty() {
            "none".to_string()
        } else {
            required.join(", ")
        },
        if optional.is_empty() {
            "none".to_string()
        } else {
            optional.join(", ")
        }
    )
}

pub fn try_parse_tool_args<T: DeserializeOwned>(args: &Value) -> Result<T, serde_json::Error> {
    let first_error = match T::deserialize(args) {
        Ok(parsed) => return Ok(parsed),
        Err(e) => e,
    };
    coerce_json_strings(args)
        .and_then(|coerced| T::deserialize(&coerced).ok())
        .ok_or(first_error)
}

fn coerce_scalar_string(text: &str, kind: &str) -> Option<Value> {
    match (kind, text.trim()) {
        ("boolean", "true") => Some(Value::Bool(true)),
        ("boolean", "false") => Some(Value::Bool(false)),
        ("integer", digits) => digits.parse::<u64>().ok().map(Value::from),
        _ => None,
    }
}

/// Models sometimes quote booleans and integers (`"true"`, `"30"`); convert them where the schema expects that type.
fn coerce_to_schema(value: &mut Value, schema: &Value) -> bool {
    let kind = schema["type"].as_str().unwrap_or_default();
    match value {
        Value::String(text) => match coerce_scalar_string(text, kind) {
            Some(coerced) => {
                *value = coerced;
                true
            }
            None => false,
        },
        Value::Object(map) => {
            map.iter_mut()
                .map(|(key, field)| coerce_to_schema(field, &schema["properties"][key.as_str()]))
                .filter(|&changed| changed)
                .count()
                > 0
        }
        Value::Array(items) => {
            items
                .iter_mut()
                .map(|item| coerce_to_schema(item, &schema["items"]))
                .filter(|&changed| changed)
                .count()
                > 0
        }
        _ => false,
    }
}

fn invalid_arguments(tool: &str, error: &serde_json::Error, schema: &Value) -> ToolResult {
    let error = error.to_string();
    let (shown, cut) = truncate_chars(&error, MAX_ERROR_CHARS);
    let ellipsis = if cut { "..." } else { "" };
    ToolResult::error(format!(
        "Invalid arguments for {tool}: {shown}{ellipsis}. Pass a JSON object matching the schema ({}); arrays and objects must not be JSON-encoded strings.",
        expected_shape(schema)
    ))
}

fn is_unparsed_raw_arguments(args: &Value) -> bool {
    args.as_object()
        .is_some_and(|map| map.len() == 1 && map.get("raw").is_some_and(Value::is_string))
}

pub fn parse_tool_args<T: DeserializeOwned>(
    tool: &str,
    args: Value,
    schema: impl FnOnce() -> Value,
) -> Result<T, ToolResult> {
    if is_unparsed_raw_arguments(&args) {
        return Err(ToolResult::error(format!(
            "Invalid arguments for {tool}: the arguments were not valid JSON (the call may have been cut off). Pass a complete JSON object matching the tool schema."
        )));
    }
    let error = match try_parse_tool_args(&args) {
        Ok(parsed) => return Ok(parsed),
        Err(error) => error,
    };
    let schema = schema();
    let mut coerced = coerce_json_strings(&args).unwrap_or(args);
    if coerce_to_schema(&mut coerced, &schema)
        && let Ok(parsed) = T::deserialize(&coerced)
    {
        return Ok(parsed);
    }
    Err(invalid_arguments(tool, &error, &schema))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Args {
        path: String,
        edits: Vec<Value>,
        #[serde(default)]
        limit: Option<u64>,
    }

    fn schema() -> Value {
        json!({ "properties": { "path": {}, "edits": {}, "limit": {} }, "required": ["path", "edits"] })
    }

    #[test]
    fn json_encoded_nested_values_are_decoded() {
        let cases = [
            json!({ "path": "a", "edits": "[{\"oldText\":\"x\"}]" }),
            normalize_tool_arguments(json!("{\"path\": \"a\", \"edits\": [{\"oldText\": \"x\"}]}")),
            json!({ "path": "a", "edits": [{ "oldText": "x" }] }),
        ];
        for args in cases {
            let parsed: Args = parse_tool_args("edit", args.clone(), schema).unwrap();
            assert_eq!(parsed.edits, [json!({ "oldText": "x" })], "{args}");
        }
    }

    #[test]
    fn only_json_encoded_objects_are_normalized() {
        let cases = [
            (json!("{\"path\": \"a\"}"), json!({ "path": "a" })),
            (json!("[1]"), json!("[1]")),
            (json!("plain"), json!("plain")),
            (json!({ "path": "{x}" }), json!({ "path": "{x}" })),
        ];
        for (input, expected) in cases {
            assert_eq!(normalize_tool_arguments(input.clone()), expected, "{input}");
        }
    }

    #[test]
    fn plain_strings_that_only_look_like_json_stay_strings() {
        let parsed: Args = parse_tool_args("edit", json!({ "path": "[draft]", "edits": [] }), schema).unwrap();
        assert_eq!(parsed.path, "[draft]");
    }

    #[test]
    fn unrecoverable_arguments_name_the_expected_fields() {
        let err = parse_tool_args::<Args>("edit", json!({ "path": "a" }), schema).unwrap_err();
        assert!(err.is_error);
        assert!(
            err.content
                .contains("Invalid arguments for edit: missing field `edits`"),
            "{}",
            err.content
        );
        assert!(
            err.content.contains("required: path, edits; optional: limit"),
            "{}",
            err.content
        );
    }

    #[test]
    fn raw_unparsed_arguments_are_rejected_even_when_every_field_is_optional() {
        #[derive(Debug, serde::Deserialize)]
        struct AllOptional {}
        let optional_schema = || json!({ "type": "object", "properties": { "limit": { "type": "integer" } } });
        let err =
            parse_tool_args::<AllOptional>("ls", json!({ "raw": "{\"path\": \"a" }), optional_schema).unwrap_err();
        assert!(err.is_error);
        assert!(err.content.contains("not valid JSON"), "{}", err.content);

        assert!(parse_tool_args::<AllOptional>("ls", json!({}), optional_schema).is_ok());
    }

    #[test]
    fn long_serde_errors_are_truncated_on_a_char_boundary() {
        let long_value = "é".repeat(2_000);
        let args = json!({ "path": "a", "edits": [], "limit": long_value });
        let err = parse_tool_args::<Args>("edit", args, schema).unwrap_err();
        let detail = err
            .content
            .strip_prefix("Invalid arguments for edit: ")
            .and_then(|rest| rest.split_once(". Pass a JSON object"))
            .and_then(|(detail, _)| detail.strip_suffix("..."))
            .unwrap_or_else(|| panic!("{}", err.content));
        assert_eq!(detail.chars().count(), MAX_ERROR_CHARS);
        assert!(err.content.contains("required: path, edits; optional: limit"));
    }

    #[test]
    fn quoted_booleans_and_integers_are_coerced_where_the_schema_expects_them() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct Typed {
            pattern: String,
            hidden: Option<bool>,
            limit: Option<usize>,
            edits: Vec<Entry>,
        }
        #[derive(Debug, Deserialize, PartialEq)]
        struct Entry {
            #[serde(rename = "replaceAll")]
            replace_all: Option<bool>,
        }
        let typed_schema = || {
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "hidden": { "type": "boolean" },
                    "limit": { "type": "integer" },
                    "edits": { "type": "array", "items": { "type": "object", "properties": { "replaceAll": { "type": "boolean" } } } }
                }
            })
        };
        let args =
            json!({ "pattern": "true", "hidden": "true", "limit": " 20 ", "edits": [{ "replaceAll": "false" }] });
        let parsed: Typed = parse_tool_args("fd", args, typed_schema).unwrap();
        assert_eq!(
            parsed,
            Typed {
                pattern: "true".into(),
                hidden: Some(true),
                limit: Some(20),
                edits: vec![Entry {
                    replace_all: Some(false)
                }],
            }
        );

        for bad in [
            json!({ "pattern": "x", "hidden": "yes", "edits": [] }),
            json!({ "pattern": "x", "limit": "-1", "edits": [] }),
        ] {
            let err = parse_tool_args::<Typed>("fd", bad.clone(), typed_schema).unwrap_err();
            assert!(
                err.content
                    .starts_with("Invalid arguments for fd: invalid type: string"),
                "{bad}: {}",
                err.content
            );
        }
    }

    #[test]
    fn try_parse_decodes_json_encoded_fields_without_consuming_args() {
        let args = json!({ "path": "a", "edits": "[1]" });
        let parsed: Args = try_parse_tool_args(&args).unwrap();
        assert_eq!(parsed.edits, [json!(1)]);
        assert!(try_parse_tool_args::<Args>(&json!({ "path": 1 })).is_err());
    }
}
