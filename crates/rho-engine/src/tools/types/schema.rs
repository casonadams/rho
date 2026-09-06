use schemars::JsonSchema;

/// Normalize a generated JSON Schema in place for provider compatibility.
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

fn register_definition_keys(
    key: &str,
    submap: &serde_json::Map<String, serde_json::Value>,
    defs: &mut std::collections::HashMap<String, serde_json::Value>,
) {
    for (name, def) in submap {
        defs.insert(format!("#/{key}/{name}"), def.clone());
        defs.insert(format!("#/$defs/{name}"), def.clone());
        defs.insert(format!("#/definitions/{name}"), def.clone());
        defs.insert(name.clone(), def.clone());
    }
}

fn collect_object_defs(
    map: &serde_json::Map<String, serde_json::Value>,
    defs: &mut std::collections::HashMap<String, serde_json::Value>,
) {
    for key in ["$defs", "definitions"] {
        if let Some(serde_json::Value::Object(submap)) = map.get(key) {
            register_definition_keys(key, submap, defs);
        }
    }
    for subval in map.values() {
        collect_definitions(subval, defs);
    }
}

fn collect_definitions(value: &serde_json::Value, defs: &mut std::collections::HashMap<String, serde_json::Value>) {
    match value {
        serde_json::Value::Object(map) => collect_object_defs(map, defs),
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_definitions(item, defs);
            }
        }
        _ => {}
    }
}

fn try_inline_ref(
    map: &serde_json::Map<String, serde_json::Value>,
    defs: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<serde_json::Value> {
    let ref_target = map.get("$ref")?.as_str()?;
    let mut inlined = defs.get(ref_target)?.clone();
    inline_refs(&mut inlined, defs);
    Some(inlined)
}

fn inline_object_refs(
    map: &mut serde_json::Map<String, serde_json::Value>,
    defs: &std::collections::HashMap<String, serde_json::Value>,
) {
    for subval in map.values_mut() {
        inline_refs(subval, defs);
    }
}

fn inline_refs(value: &mut serde_json::Value, defs: &std::collections::HashMap<String, serde_json::Value>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(inlined) = try_inline_ref(map, defs) {
                *value = inlined;
            } else {
                inline_object_refs(map, defs);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                inline_refs(item, defs);
            }
        }
        _ => {}
    }
}

fn clean_array_type(map: &mut serde_json::Map<String, serde_json::Value>) {
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
}

fn try_clean_any_of(map: &mut serde_json::Map<String, serde_json::Value>) -> Option<serde_json::Value> {
    let arr = map.get("anyOf")?.as_array()?;
    let non_null: Vec<_> = arr
        .iter()
        .filter(|item| item.as_object().and_then(|o| o.get("type")).and_then(|t| t.as_str()) != Some("null"))
        .cloned()
        .collect();
    if non_null.len() == 1 {
        let mut single = non_null[0].clone();
        clean_schema(&mut single);
        Some(single)
    } else {
        None
    }
}

fn clean_object_schema(map: &mut serde_json::Map<String, serde_json::Value>) {
    if map.get("default") == Some(&serde_json::Value::Null) {
        map.remove("default");
    }
    clean_array_type(map);
    for subval in map.values_mut() {
        clean_schema(subval);
    }
}

fn clean_schema(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Bool(true) => *value = serde_json::Value::Object(serde_json::Map::new()),
        serde_json::Value::Object(map) => {
            if let Some(single) = try_clean_any_of(map) {
                *value = single;
            } else {
                clean_object_schema(map);
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
