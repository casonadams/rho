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
            .ok_or_else(|| format!("unclosed '{{{{' in: {}", preview(template)))?;
        let path = after[..end].trim();
        if path.is_empty() {
            return Err(format!("empty '{{{{ }}}}' placeholder in: {}", preview(template)));
        }
        refs.push(path.to_string());
        rest = &after[end + CLOSE.len()..];
    }
    Ok(refs)
}

pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<Value>;

/// Fills placeholders; references with no value render as empty text.
#[must_use]
pub fn render(template: &str, scope: &Value) -> String {
    render_with(template, &|path| lookup(scope, path).cloned())
}

#[must_use]
pub fn render_with(template: &str, resolve: Resolver<'_>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find(CLOSE) else {
            out.push_str(&rest[start..]);
            return out;
        };
        if let Some(value) = resolve(after[..end].trim()) {
            out.push_str(&value_to_text(&value));
        }
        rest = &after[end + CLOSE.len()..];
    }
    out.push_str(rest);
    out
}

/// A template that is exactly one placeholder yields the referenced value itself (`null` when missing).
#[must_use]
pub fn evaluate(template: &str, scope: &Value) -> Value {
    evaluate_with(template, &|path| lookup(scope, path).cloned())
}

#[must_use]
pub fn evaluate_with(template: &str, resolve: Resolver<'_>) -> Value {
    let trimmed = template.trim();
    let single = trimmed
        .strip_prefix(OPEN)
        .and_then(|rest| rest.strip_suffix(CLOSE))
        .filter(|inner| !inner.contains(OPEN) && !inner.contains(CLOSE));
    match single {
        Some(path) => resolve(path.trim()).unwrap_or(Value::Null),
        None => Value::String(render_with(template, resolve)),
    }
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
        Value::String(s) => !matches!(s.trim(), "" | "false" | "0"),
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
        let refs = references("Scan {{ slice }} for {{profile}}.").unwrap();
        assert_eq!(refs, vec!["slice", "profile"]);
    }

    #[test]
    fn rejects_unclosed_and_empty_placeholders() {
        assert!(references("oops {{ item").unwrap_err().contains("unclosed"));
        assert!(references("oops {{ }}").unwrap_err().contains("empty"));
    }

    #[test]
    fn renders_strings_raw_structures_as_json_and_missing_as_empty() {
        let scope = json!({"profile": "security", "scan": ["a", "b"], "round": {"index": 2}});
        assert_eq!(
            render("{{profile}} r{{round.index}} {{scan.1}}", &scope),
            "security r2 b"
        );
        assert_eq!(render("{{scan}}", &scope), "[\n  \"a\",\n  \"b\"\n]");
        assert_eq!(render("[{{verify.output}}]", &scope), "[]");
    }

    #[test]
    fn evaluate_passes_single_references_through() {
        let scope = json!({"list": [1, 2], "ok": true});
        assert_eq!(evaluate("{{ list }}", &scope), json!([1, 2]));
        assert_eq!(evaluate("{{ok}}", &scope), json!(true));
        assert_eq!(evaluate("{{missing}}", &scope), Value::Null);
        assert_eq!(evaluate("n={{ok}}", &scope), json!("n=true"));
    }

    #[test]
    fn truthiness_follows_emptiness() {
        let cases = [
            (json!(null), false),
            (json!(false), false),
            (json!(0), false),
            (json!(""), false),
            (json!("false"), false),
            (json!("0"), false),
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
