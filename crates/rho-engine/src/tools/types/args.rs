use super::ToolResult;
use serde::de::DeserializeOwned;
use serde_json::Value;

const MAX_ERROR_CHARS: usize = 300;

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

fn truncate_chars(text: &str, max_chars: usize) -> &str {
    text.char_indices().nth(max_chars).map_or(text, |(end, _)| &text[..end])
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

pub fn parse_tool_args<T: DeserializeOwned>(
    tool: &str,
    args: Value,
    schema: impl FnOnce() -> Value,
) -> Result<T, ToolResult> {
    try_parse_tool_args(&args).map_err(|error| {
        let error = error.to_string();
        let shown = truncate_chars(&error, MAX_ERROR_CHARS);
        let ellipsis = if shown.len() < error.len() { "..." } else { "" };
        ToolResult::error(format!(
            "Invalid arguments for {tool}: {shown}{ellipsis}. Pass a JSON object matching the schema ({}); arrays and objects must not be JSON-encoded strings.",
            expected_shape(&schema())
        ))
    })
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
    fn try_parse_decodes_json_encoded_fields_without_consuming_args() {
        let args = json!({ "path": "a", "edits": "[1]" });
        let parsed: Args = try_parse_tool_args(&args).unwrap();
        assert_eq!(parsed.edits, [json!(1)]);
        assert!(try_parse_tool_args::<Args>(&json!({ "path": 1 })).is_err());
    }
}
