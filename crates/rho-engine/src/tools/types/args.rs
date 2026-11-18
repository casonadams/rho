use super::ToolResult;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn decode_json_text(value: &Value) -> Option<Value> {
    let text = value.as_str()?.trim();
    if !(text.starts_with('{') || text.starts_with('[')) {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// Models sometimes send nested arrays or objects as JSON-encoded strings; decode those before giving up.
fn coerce_json_strings(args: &Value) -> Option<Value> {
    if let Some(decoded) = decode_json_text(args) {
        return Some(decoded);
    }
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

pub fn parse_tool_args<T: DeserializeOwned>(
    tool: &str,
    args: Value,
    schema: impl FnOnce() -> Value,
) -> Result<T, ToolResult> {
    let first_error = match serde_json::from_value::<T>(args.clone()) {
        Ok(parsed) => return Ok(parsed),
        Err(e) => e,
    };
    if let Some(parsed) = coerce_json_strings(&args).and_then(|coerced| serde_json::from_value::<T>(coerced).ok()) {
        return Ok(parsed);
    }
    Err(ToolResult::error(format!(
        "Invalid arguments for {tool}: {first_error}. Pass a JSON object matching the schema ({}); arrays and objects must not be JSON-encoded strings.",
        expected_shape(&schema())
    )))
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
            json!("{\"path\": \"a\", \"edits\": [{\"oldText\": \"x\"}]}"),
            json!({ "path": "a", "edits": [{ "oldText": "x" }] }),
        ];
        for args in cases {
            let parsed: Args = parse_tool_args("edit", args.clone(), schema).unwrap();
            assert_eq!(parsed.edits, [json!({ "oldText": "x" })], "{args}");
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
}
