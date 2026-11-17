use serde_json::Value;

const OPEN: &str = "{{";
const CLOSE: &str = "}}";

pub fn references(template: &str) -> Result<Vec<String>, String> {
    let mut refs = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let end = after
            .find(CLOSE)
            .ok_or_else(|| format!("unclosed '{{{{' in template: {}", preview(template)))?;
        let path = after[..end].trim();
        if path.is_empty() {
            return Err(format!(
                "empty '{{{{ }}}}' placeholder in template: {}",
                preview(template)
            ));
        }
        refs.push(path.to_string());
        rest = &after[end + CLOSE.len()..];
    }
    Ok(refs)
}

pub fn render(template: &str, scope: &Value) -> Result<String, String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + OPEN.len()..];
        let end = after
            .find(CLOSE)
            .ok_or_else(|| format!("unclosed '{{{{' in template: {}", preview(template)))?;
        let path = after[..end].trim();
        let value = lookup(scope, path).ok_or_else(|| format!("'{{{{{path}}}}}' has no value"))?;
        out.push_str(&value_to_text(value));
        rest = &after[end + CLOSE.len()..];
    }
    out.push_str(rest);
    Ok(out)
}

pub fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(root, |value, segment| match value {
        Value::Object(map) => map.get(segment),
        Value::Array(items) => segment.parse::<usize>().ok().and_then(|i| items.get(i)),
        _ => None,
    })
}

#[must_use]
pub fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        Value::Bool(_) | Value::Number(_) => value.to_string(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string_pretty(value).unwrap_or_default(),
    }
}

#[must_use]
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.trim().is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn preview(template: &str) -> String {
    let line = template.lines().next().unwrap_or_default();
    line.chars().take(60).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_references_with_whitespace() {
        let refs = references("Scan {{ item.paths }} for {{inputs.profile}}.").unwrap();
        assert_eq!(refs, vec!["item.paths", "inputs.profile"]);
    }

    #[test]
    fn rejects_unclosed_and_empty_placeholders() {
        assert!(references("oops {{ item").unwrap_err().contains("unclosed"));
        assert!(references("oops {{ }}").unwrap_err().contains("empty"));
    }

    #[test]
    fn renders_strings_raw_and_structures_as_json() {
        let scope = json!({"inputs": {"profile": "security"}, "item": {"paths": ["src", "lib"]}, "round": 2});
        let out = render("{{inputs.profile}} r{{round}} {{item.paths.1}}", &scope).unwrap();
        assert_eq!(out, "security r2 lib");
        let listed = render("{{item.paths}}", &scope).unwrap();
        assert_eq!(listed, "[\n  \"src\",\n  \"lib\"\n]");
    }

    #[test]
    fn render_fails_on_missing_value() {
        assert!(render("{{nope.x}}", &json!({})).unwrap_err().contains("nope.x"));
    }

    #[test]
    fn truthiness_follows_emptiness() {
        let cases = [
            (json!(null), false),
            (json!(false), false),
            (json!(0), false),
            (json!(""), false),
            (json!([]), false),
            (json!({}), false),
            (json!(true), true),
            (json!(1), true),
            (json!("x"), true),
            (json!([1]), true),
            (json!({"a": 1}), true),
        ];
        for (value, expected) in cases {
            assert_eq!(is_truthy(&value), expected, "{value}");
        }
    }
}
