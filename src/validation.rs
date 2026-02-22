use jsonschema::Validator;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

pub fn validate(schema: &Validator, json: &Value) -> ValidationResult {
    let errors: Vec<String> = schema
        .iter_errors(json)
        .map(|e| {
            let instance_path = e.instance_path().to_string();
            let error_description = e.to_string();
            if instance_path.is_empty() {
                error_description
            } else {
                format!("{}: {}", instance_path, error_description)
            }
        })
        .collect();
    if errors.is_empty() {
        ValidationResult {
            valid: true,
            errors: vec![],
        }
    } else {
        ValidationResult {
            valid: false,
            errors,
        }
    }
}
