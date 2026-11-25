use serde::de::{self, Deserializer};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BindingError {
    #[error("invalid JSON pointer '{pointer}': {reason}")]
    InvalidPointer { pointer: String, reason: String },
}

/// A component property that holds either a literal value or an RFC 6901 JSON-Pointer
/// reference into the surface's `data_model`.
///
/// Serializes literals directly and path bindings as `{"path": "/a/b"}`.
/// Deserializes literals, `{"path": "/a/b"}`, and `"@/a/b"` string shorthand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundValue<T> {
    Literal(T),
    Path(String),
}

pub type BoundString = BoundValue<String>;

impl<T> BoundValue<T> {
    pub fn literal(value: impl Into<T>) -> Self {
        Self::Literal(value.into())
    }

    pub fn path(pointer: impl Into<String>) -> Self {
        Self::Path(pointer.into())
    }

    pub fn as_path(&self) -> Option<&str> {
        match self {
            Self::Path(path) => Some(path.as_str()),
            Self::Literal(_) => None,
        }
    }
}

impl From<&str> for BoundValue<String> {
    fn from(value: &str) -> Self {
        if let Some(pointer) = value.strip_prefix("@/") {
            Self::Path(format!("/{pointer}"))
        } else {
            Self::Literal(value.to_string())
        }
    }
}

impl From<String> for BoundValue<String> {
    fn from(value: String) -> Self {
        if let Some(pointer) = value.strip_prefix("@/") {
            Self::Path(format!("/{pointer}"))
        } else {
            Self::Literal(value)
        }
    }
}

#[derive(Serialize)]
struct PathRef<'a> {
    path: &'a str,
}

#[derive(Deserialize)]
struct PathRefOwned {
    path: String,
}

impl<T: Serialize> Serialize for BoundValue<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Literal(value) => value.serialize(serializer),
            Self::Path(path) => PathRef { path }.serialize(serializer),
        }
    }
}

impl<'de, T: de::DeserializeOwned> Deserialize<'de> for BoundValue<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if let Value::Object(ref map) = value
            && map.len() == 1
            && map.contains_key("path")
        {
            let parsed: PathRefOwned = serde_json::from_value(value).map_err(de::Error::custom)?;
            return Ok(Self::Path(parsed.path));
        }
        if let Value::String(ref text) = value
            && let Some(pointer) = text.strip_prefix("@/")
        {
            return Ok(Self::Path(format!("/{pointer}")));
        }
        T::deserialize(value).map(Self::Literal).map_err(de::Error::custom)
    }
}

/// Resolves an RFC 6901 JSON Pointer against `data`.
///
/// An empty pointer `""` resolves to the root `data` value.
pub fn resolve_pointer<'a>(data: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        return Some(data);
    }
    data.pointer(pointer)
}

/// Resolves a `BoundString` against `data`, returning an empty string when a bound path is missing
/// or null.
pub fn resolve_string(data: &Value, bound: &BoundString) -> String {
    match bound {
        BoundValue::Literal(text) => text.clone(),
        BoundValue::Path(pointer) => match resolve_pointer(data, pointer) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Number(num)) => num.to_string(),
            Some(Value::Bool(flag)) => flag.to_string(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        },
    }
}

/// Resolves a typed `BoundValue<T>` against `data`.
pub fn resolve_typed<T: Clone + de::DeserializeOwned>(data: &Value, bound: &BoundValue<T>) -> Option<T> {
    match bound {
        BoundValue::Literal(value) => Some(value.clone()),
        BoundValue::Path(pointer) => {
            resolve_pointer(data, pointer).and_then(|v| serde_json::from_value(v.clone()).ok())
        }
    }
}

/// Patches `root` at an optional RFC 6901 JSON Pointer `path` with `value`.
///
/// When `path` is `None` or `Some("")`, replaces `root` wholesale.
/// Intermediate missing or `null` containers are created as JSON objects.
pub fn apply_data_patch(root: &mut Value, path: Option<&str>, value: Value) -> Result<(), BindingError> {
    let Some(pointer) = path else {
        *root = value;
        return Ok(());
    };
    if pointer.is_empty() {
        *root = value;
        return Ok(());
    }
    if !pointer.starts_with('/') {
        return Err(BindingError::InvalidPointer {
            pointer: pointer.to_string(),
            reason: "JSON pointer must be empty or start with '/'".to_string(),
        });
    }

    let segments: Vec<String> = pointer[1..].split('/').map(unescape_pointer_segment).collect();
    let Some((last, parents)) = segments.split_last() else {
        *root = value;
        return Ok(());
    };

    let mut current = root;
    for segment in parents {
        current = descend_or_create_object(current, segment, pointer)?;
    }

    assign_leaf(current, last, value, pointer)
}

fn descend_or_create_object<'a>(
    current: &'a mut Value,
    segment: &str,
    full_pointer: &str,
) -> Result<&'a mut Value, BindingError> {
    if current.is_null() {
        *current = Value::Object(Map::new());
    }
    match current {
        Value::Object(map) => Ok(map
            .entry(segment.to_string())
            .or_insert_with(|| Value::Object(Map::new()))),
        Value::Array(items) => {
            let idx = parse_array_index(segment, items.len(), full_pointer)?;
            Ok(&mut items[idx])
        }
        _ => Err(BindingError::InvalidPointer {
            pointer: full_pointer.to_string(),
            reason: format!("cannot traverse through scalar at '{segment}'"),
        }),
    }
}

fn assign_leaf(current: &mut Value, segment: &str, value: Value, full_pointer: &str) -> Result<(), BindingError> {
    if current.is_null() {
        *current = Value::Object(Map::new());
    }
    match current {
        Value::Object(map) => {
            map.insert(segment.to_string(), value);
            Ok(())
        }
        Value::Array(items) => {
            if segment == "-" {
                items.push(value);
                return Ok(());
            }
            let idx = parse_array_index(segment, items.len(), full_pointer)?;
            items[idx] = value;
            Ok(())
        }
        _ => Err(BindingError::InvalidPointer {
            pointer: full_pointer.to_string(),
            reason: format!("cannot set '{segment}' on scalar value"),
        }),
    }
}

fn parse_array_index(segment: &str, len: usize, full_pointer: &str) -> Result<usize, BindingError> {
    let idx = segment.parse::<usize>().map_err(|_| BindingError::InvalidPointer {
        pointer: full_pointer.to_string(),
        reason: format!("invalid array index '{segment}'"),
    })?;
    if idx >= len {
        return Err(BindingError::InvalidPointer {
            pointer: full_pointer.to_string(),
            reason: format!("array index {idx} out of bounds (len {len})"),
        });
    }
    Ok(idx)
}

fn unescape_pointer_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    let mut chars = segment.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.next() {
                Some('1') => out.push('/'),
                Some('0') => out.push('~'),
                Some(other) => {
                    out.push('~');
                    out.push(other);
                }
                None => out.push('~'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bound_string_deserializes_literal_object_path_and_at_shorthand() {
        let literal: BoundString = serde_json::from_str(r#""hello""#).unwrap();
        assert_eq!(literal, BoundString::literal("hello"));

        let at_user: BoundString = serde_json::from_str(r#""@alice""#).unwrap();
        assert_eq!(at_user, BoundString::literal("@alice"));

        let path_obj: BoundString = serde_json::from_str(r#"{"path":"/user/name"}"#).unwrap();
        assert_eq!(path_obj, BoundString::path("/user/name"));

        let shorthand: BoundString = serde_json::from_str(r#""@/user/name""#).unwrap();
        assert_eq!(shorthand, BoundString::path("/user/name"));

        let serialized = serde_json::to_string(&shorthand).unwrap();
        assert_eq!(serialized, r#"{"path":"/user/name"}"#);
    }

    #[test]
    fn rfc6901_pointer_resolution_and_escaping() {
        let data = json!({
            "a/b": "slash",
            "m~n": "tilde",
            "~01": "literal_tilde01",
            "nested": {
                "count": 42,
                "active": true
            }
        });

        assert_eq!(resolve_string(&data, &BoundString::path("/a~1b")), "slash");
        assert_eq!(resolve_string(&data, &BoundString::path("/m~0n")), "tilde");
        assert_eq!(resolve_string(&data, &BoundString::path("/~001")), "literal_tilde01");
        assert_eq!(resolve_string(&data, &BoundString::path("/nested/count")), "42");
        assert_eq!(resolve_string(&data, &BoundString::path("/nested/active")), "true");
        assert_eq!(resolve_string(&data, &BoundString::path("/missing")), "");
    }

    #[test]
    fn apply_data_patch_creates_intermediate_objects_and_supports_escaping() {
        let mut root = json!({"existing": 1});
        apply_data_patch(&mut root, Some("/user/profile/a~1b"), json!("ok")).unwrap();
        apply_data_patch(&mut root, Some("/~01"), json!("tilde_one")).unwrap();

        assert_eq!(
            root,
            json!({
                "existing": 1,
                "user": { "profile": { "a/b": "ok" } },
                "~1": "tilde_one"
            })
        );

        apply_data_patch(&mut root, None, json!({"reset": true})).unwrap();
        assert_eq!(root, json!({"reset": true}));
    }

    #[test]
    fn apply_data_patch_rejects_invalid_pointers_and_scalar_traversal() {
        let mut root = json!({"scalar": "text"});
        assert!(apply_data_patch(&mut root, Some("no_leading_slash"), json!(1)).is_err());
        assert!(apply_data_patch(&mut root, Some("/scalar/child"), json!(1)).is_err());
    }
}
