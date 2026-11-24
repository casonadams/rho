use serde_json::{Map, Value};

const OPEN: &str = "{{";
const CLOSE: &str = "}}";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Filter {
    Compact,
    Count,
    Json,
    Default(String),
    Pluck(String),
    CountBy(String),
}

impl Filter {
    fn parse(spec: &str) -> Result<Self, String> {
        let (name, arg) = spec.split_once(':').map_or((spec, None), |(n, a)| (n, Some(a.trim())));
        match (name.trim(), arg) {
            ("compact", None) => Ok(Self::Compact),
            ("count", None) => Ok(Self::Count),
            ("json", None) => Ok(Self::Json),
            ("default", Some(path)) if !path.is_empty() => Ok(Self::Default(path.to_string())),
            ("pluck", Some(field)) if !field.is_empty() => Ok(Self::Pluck(field.to_string())),
            ("count_by", Some(field)) if !field.is_empty() => Ok(Self::CountBy(field.to_string())),
            _ => Err(format!(
                "unknown filter '{}' (use compact, count, json, default:<ref>, pluck:<field>, count_by:<field>)",
                spec.trim()
            )),
        }
    }

    fn apply(&self, value: Value, resolve: Resolver<'_>) -> Value {
        match self {
            Self::Default(path) if !is_truthy(&value) => resolve(path).unwrap_or(Value::Null),
            Self::Default(_) => value,
            Self::Compact => match value {
                Value::Array(items) => Value::Array(items.into_iter().filter(|v| !v.is_null()).collect()),
                other => other,
            },
            Self::Count => match &value {
                Value::Array(items) => items.len().into(),
                Value::Object(map) => map.len().into(),
                Value::Null => 0.into(),
                _ => 1.into(),
            },
            Self::Json => Value::String(value.to_string()),
            Self::Pluck(field) => Value::Array(items_of(&value).filter_map(|i| lookup(i, field).cloned()).collect()),
            Self::CountBy(field) => {
                let mut counts = Map::new();
                for key in items_of(&value).filter_map(|i| lookup(i, field)).map(value_to_text) {
                    let seen = counts.get(&key).and_then(Value::as_u64).unwrap_or(0);
                    counts.insert(key, (seen + 1).into());
                }
                Value::Object(counts)
            }
        }
    }
}

fn items_of(value: &Value) -> impl Iterator<Item = &Value> {
    match value {
        Value::Array(items) => items.iter(),
        _ => [].iter(),
    }
}

/// Splits `path | filter | filter:arg` into the path and its parsed filters.
fn parse_expr(expr: &str) -> Result<(&str, Vec<Filter>), String> {
    let mut parts = expr.split('|');
    let path = parts.next().unwrap_or_default().trim();
    let filters = parts.map(Filter::parse).collect::<Result<Vec<_>, _>>()?;
    Ok((path, filters))
}

fn resolve_expr(expr: &str, resolve: Resolver<'_>) -> Option<Value> {
    let (path, filters) = parse_expr(expr).ok()?;
    let value = resolve(path);
    if filters.is_empty() {
        return value;
    }
    Some(
        filters
            .iter()
            .fold(value.unwrap_or(Value::Null), |v, f| f.apply(v, resolve)),
    )
}

pub fn references(template: &str) -> Result<Vec<String>, String> {
    let mut refs = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let end = after
            .find(CLOSE)
            .ok_or_else(|| format!("unclosed '{{{{' in: {}", preview(template)))?;
        let expr = after[..end].trim();
        let (path, filters) = parse_expr(expr).map_err(|e| format!("{e} in: {}", preview(template)))?;
        if path.is_empty() {
            return Err(format!("empty '{{{{ }}}}' placeholder in: {}", preview(template)));
        }
        refs.push(path.to_string());
        refs.extend(filters.into_iter().filter_map(|f| match f {
            Filter::Default(fallback) => Some(fallback),
            _ => None,
        }));
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
        if let Some(value) = resolve_expr(after[..end].trim(), resolve) {
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
        Some(expr) => resolve_expr(expr.trim(), resolve).unwrap_or(Value::Null),
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
    fn filters_transform_values_in_order() {
        let scope = json!({"f": [{"p": "P1"}, null, {"p": "P2"}, {"p": "P1"}], "list": [1, 2], "empty": []});
        let cases = [
            ("{{f | count}}", json!(4)),
            ("{{f | compact | count}}", json!(3)),
            ("{{f | compact | pluck:p}}", json!(["P1", "P2", "P1"])),
            ("{{f | compact | count_by:p}}", json!({"P1": 2, "P2": 1})),
            ("{{missing | count}}", json!(0)),
            ("{{missing | default:list}}", json!([1, 2])),
            ("{{empty | default:list}}", json!([1, 2])),
            ("{{list | default:empty}}", json!([1, 2])),
            ("{{f | compact | pluck:p | json}}", json!("[\"P1\",\"P2\",\"P1\"]")),
        ];
        for (template, expected) in cases {
            assert_eq!(evaluate(template, &scope), expected, "{template}");
        }
        assert_eq!(render("n={{f | count}}", &scope), "n=4");
    }

    #[test]
    fn references_report_the_path_and_reject_unknown_filters() {
        assert_eq!(references("{{ a.b | count }}").unwrap(), vec!["a.b"]);
        assert_eq!(references("{{ a | default:b.c }}").unwrap(), vec!["a", "b.c"]);
        assert!(
            references("{{ a | shout }}")
                .unwrap_err()
                .contains("unknown filter 'shout'")
        );
        assert!(references("{{ a | pluck }}").is_err());
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
