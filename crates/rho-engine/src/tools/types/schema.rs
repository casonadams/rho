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
