use serde_json::Value;

pub const MAX_SCHEMA_BYTES: usize = 262_144;
pub const MAX_SCHEMA_DIAGNOSTIC_BYTES: usize = 512;

pub struct CompiledSchema {
    validator: jsonschema::Validator,
}

impl CompiledSchema {
    pub fn compile(schema: &Value) -> Result<Self, SchemaError> {
        let encoded_size = serde_json::to_vec(schema).map_err(|_| SchemaError::Malformed)?.len();
        if encoded_size > MAX_SCHEMA_BYTES {
            return Err(SchemaError::TooLarge);
        }
        let validator = jsonschema::validator_for(schema).map_err(|_| SchemaError::InvalidOrUnsupported)?;
        Ok(Self { validator })
    }

    pub fn compile_json(schema: &str) -> Result<Self, SchemaError> {
        if schema.len() > MAX_SCHEMA_BYTES {
            return Err(SchemaError::TooLarge);
        }
        let schema = serde_json::from_str(schema).map_err(|_| SchemaError::Malformed)?;
        Self::compile(&schema)
    }

    pub fn validate(&self, instance: &Value) -> Result<(), SchemaError> {
        self.validator.validate(instance).map_err(|error| {
            let mut path = error.instance_path().to_string();
            if path.len() > MAX_SCHEMA_DIAGNOSTIC_BYTES {
                let mut boundary = MAX_SCHEMA_DIAGNOSTIC_BYTES;
                while !path.is_char_boundary(boundary) {
                    boundary -= 1;
                }
                path.truncate(boundary);
            }
            SchemaError::InstanceInvalid { path }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaError {
    #[error("JSON Schema is malformed")]
    Malformed,
    #[error("JSON Schema is invalid or uses an unsupported dialect or reference")]
    InvalidOrUnsupported,
    #[error("JSON Schema exceeds the configured size limit")]
    TooLarge,
    #[error("arguments do not match JSON Schema at {path}")]
    InstanceInvalid { path: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_arbitrary_plugin_arguments() {
        let schema = CompiledSchema::compile(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"count": {"type": "integer", "minimum": 1}},
            "required": ["count"],
            "additionalProperties": false
        }))
        .unwrap();
        schema.validate(&json!({"count": 2})).unwrap();
        assert_eq!(
            schema.validate(&json!({"count": 0, "secret": "credential-value"})),
            Err(SchemaError::InstanceInvalid {
                path: "/count".to_string()
            })
        );
    }

    #[test]
    fn supports_boolean_schemas() {
        CompiledSchema::compile(&json!(true))
            .unwrap()
            .validate(&json!({"anything": true}))
            .unwrap();
        assert!(
            CompiledSchema::compile(&json!(false))
                .unwrap()
                .validate(&json!(null))
                .is_err()
        );
    }

    #[test]
    fn rejects_malformed_unsupported_and_oversized_schemas() {
        assert!(matches!(CompiledSchema::compile_json("{"), Err(SchemaError::Malformed)));
        assert!(matches!(
            CompiledSchema::compile(&json!({"$schema": "https://example.invalid/schema"})),
            Err(SchemaError::InvalidOrUnsupported)
        ));
        assert!(matches!(
            CompiledSchema::compile(&json!({"$ref": "https://example.invalid/schema"})),
            Err(SchemaError::InvalidOrUnsupported)
        ));
        let oversized = format!("{{\"description\":\"{}\"}}", "x".repeat(MAX_SCHEMA_BYTES));
        assert!(matches!(
            CompiledSchema::compile_json(&oversized),
            Err(SchemaError::TooLarge)
        ));
    }

    #[test]
    fn diagnostics_are_bounded_and_do_not_include_instance_values() {
        let secret = "credential-value";
        let schema = CompiledSchema::compile(&json!({"type": "integer"})).unwrap();
        let error = schema.validate(&json!(secret)).unwrap_err().to_string();
        assert!(error.len() <= MAX_SCHEMA_DIAGNOSTIC_BYTES + 64);
        assert!(!error.contains(secret));
    }
}
