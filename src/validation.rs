use jsonschema::JSONSchema;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

pub fn validate(schema: &JSONSchema, json: &Value) -> ValidationResult {
    match schema.validate(json) {
        Ok(_) => ValidationResult {
            valid: true,
            errors: vec![],
        },
        Err(errors) => {
            let error_messages: Vec<String> = errors
                .map(|e| {
                    let instance_path = e.instance_path.to_string();
                    let error_description = e.to_string();

                    if instance_path.is_empty() {
                        error_description
                    } else {
                        format!("{}: {}", instance_path, error_description)
                    }
                })
                .collect();

            ValidationResult {
                valid: false,
                errors: error_messages,
            }
        }
    }
}
