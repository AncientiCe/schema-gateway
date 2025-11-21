use axum::http::Method;
use jsonschema::JSONSchema;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};

#[derive(Clone)]
pub struct OperationValidationPlan {
    pub schema: Option<Arc<JSONSchema>>,
    pub body_required: bool,
    pub method: String,
    pub path_template: String,
    pub parameters: Vec<ParameterValidator>,
    pub response_schemas: HashMap<ResponseKey, Arc<JSONSchema>>,
}

#[derive(Clone)]
pub struct ParameterValidator {
    pub name: String,
    pub location: ParameterLocation,
    pub required: bool,
    pub schema: Option<Arc<JSONSchema>>,
    value_type: Option<PrimitiveType>,
}

impl ParameterValidator {
    pub fn coerce_value(&self, raw: &str) -> std::result::Result<Value, String> {
        match self.value_type {
            Some(PrimitiveType::Integer) => raw
                .parse::<i64>()
                .map(|v| Value::Number(v.into()))
                .map_err(|_| format!("Failed to parse integer for parameter '{}'", self.name)),
            Some(PrimitiveType::Number) => raw
                .parse::<f64>()
                .map(|v| Value::Number(serde_json::Number::from_f64(v).unwrap()))
                .map_err(|_| format!("Failed to parse number for parameter '{}'", self.name)),
            Some(PrimitiveType::Boolean) => raw
                .parse::<bool>()
                .map(Value::Bool)
                .map_err(|_| format!("Failed to parse boolean for parameter '{}'", self.name)),
            None => Ok(Value::String(raw.to_string())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResponseKey {
    Status(u16),
    Default,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PrimitiveType {
    Integer,
    Number,
    Boolean,
}

#[derive(Default)]
pub struct OpenApiCache {
    specs: HashMap<PathBuf, Arc<Value>>,
    operations: HashMap<OperationCacheKey, OperationValidationPlan>,
}

impl OpenApiCache {
    pub fn new() -> Self {
        Self {
            specs: HashMap::new(),
            operations: HashMap::new(),
        }
    }

    pub fn load_operation(
        &mut self,
        spec_path: impl AsRef<Path>,
        route_path: &str,
        method: &Method,
        operation_id: Option<&str>,
    ) -> Result<OperationValidationPlan> {
        let path_buf = spec_path.as_ref().to_path_buf();
        let spec = self.load_spec(&path_buf)?;
        let method_key = method.as_str().to_lowercase();

        let operation = find_operation(
            spec.as_ref(),
            route_path,
            &method_key,
            operation_id,
            &path_buf,
        )?;

        let cache_key = OperationCacheKey {
            spec_path: path_buf.clone(),
            method: method_key.clone(),
            path_template: operation.path.clone(),
        };

        if let Some(plan) = self.operations.get(&cache_key) {
            return Ok(plan.clone());
        }

        let schema_arc = if let Some(schema_value) = operation.schema {
            let resolved_schema = resolve_schema_value(&schema_value, spec.as_ref(), &path_buf)?;
            let compiled =
                JSONSchema::compile(&resolved_schema).map_err(|e| Error::InvalidOpenApi {
                    path: path_buf.clone(),
                    message: e.to_string(),
                })?;
            Some(Arc::new(compiled))
        } else {
            None
        };

        let parameter_validators = compile_parameter_validators(operation.parameters, &path_buf)?;
        let response_schemas = compile_response_schemas(operation.responses, &path_buf)?;

        let plan = OperationValidationPlan {
            schema: schema_arc,
            body_required: operation.body_required,
            method: method_key.to_uppercase(),
            path_template: operation.path,
            parameters: parameter_validators,
            response_schemas,
        };

        self.operations.insert(cache_key, plan.clone());
        Ok(plan)
    }

    fn load_spec(&mut self, path: &Path) -> Result<Arc<Value>> {
        if let Some(spec) = self.specs.get(path) {
            return Ok(Arc::clone(spec));
        }

        let contents = match fs::read_to_string(path) {
            Ok(data) => data,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Err(Error::OpenApiNotFound {
                        path: path.to_path_buf(),
                    });
                }
                return Err(Error::Io(e));
            }
        };

        let spec: Value = serde_yaml::from_str(&contents).map_err(|e| Error::InvalidOpenApi {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

        let arc = Arc::new(spec);
        self.specs.insert(path.to_path_buf(), Arc::clone(&arc));
        Ok(arc)
    }
}

struct OperationMatch {
    path: String,
    body_required: bool,
    schema: Option<Value>,
    parameters: Vec<ParameterSpec>,
    responses: HashMap<ResponseKey, Value>,
}

struct ParameterSpec {
    name: String,
    location: ParameterLocation,
    required: bool,
    schema: Option<Value>,
}

fn find_operation(
    spec: &Value,
    route_path: &str,
    method: &str,
    operation_id: Option<&str>,
    spec_path: &Path,
) -> Result<OperationMatch> {
    let paths = spec
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| Error::OpenApi {
            path: spec_path.to_path_buf(),
            message: "OpenAPI document missing 'paths' section".to_string(),
        })?;

    if let Some(op_id) = operation_id {
        return find_by_operation_id(paths, op_id, method, route_path, spec, spec_path);
    }

    find_by_path(paths, route_path, method, spec, spec_path)
}

fn find_by_operation_id(
    paths: &Map<String, Value>,
    target_operation_id: &str,
    method: &str,
    route_path: &str,
    spec: &Value,
    spec_path: &Path,
) -> Result<OperationMatch> {
    for (path_template, path_value) in paths {
        let resolved_path_item = resolve_reference(path_value, spec, spec_path)?;
        let path_obj = resolved_path_item
            .as_object()
            .ok_or_else(|| Error::OpenApi {
                path: spec_path.to_path_buf(),
                message: format!("Path item '{}' is not an object", path_template),
            })?;

        for operation_key in METHOD_KEYS {
            if let Some(operation_value) = path_obj.get(*operation_key) {
                let resolved_operation =
                    resolve_reference(operation_value, spec, spec_path)?.clone();
                if let Some(op_id) = resolved_operation
                    .get("operationId")
                    .and_then(Value::as_str)
                {
                    if op_id == target_operation_id {
                        if *operation_key != method {
                            return Err(Error::OpenApi {
                                path: spec_path.to_path_buf(),
                                message: format!(
                                    "operation '{}' uses HTTP method '{}', not '{}'",
                                    target_operation_id, operation_key, method
                                ),
                            });
                        }

                        if !paths_match(route_path, path_template) {
                            return Err(Error::OpenApi {
                                path: spec_path.to_path_buf(),
                                message: format!(
                                    "operation '{}' is defined at '{}' which does not match route '{}'",
                                    target_operation_id, path_template, route_path
                                ),
                            });
                        }

                        return build_operation_match(
                            path_template,
                            resolved_path_item.clone(),
                            resolved_operation,
                            spec,
                            spec_path,
                        );
                    }
                }
            }
        }
    }

    Err(Error::OpenApi {
        path: spec_path.to_path_buf(),
        message: format!("operation '{}' was not found", target_operation_id),
    })
}

fn find_by_path(
    paths: &Map<String, Value>,
    route_path: &str,
    method: &str,
    spec: &Value,
    spec_path: &Path,
) -> Result<OperationMatch> {
    for (path_template, path_value) in paths {
        if !paths_match(route_path, path_template) {
            continue;
        }

        let resolved_path = resolve_reference(path_value, spec, spec_path)?;
        let path_obj = resolved_path.as_object().ok_or_else(|| Error::OpenApi {
            path: spec_path.to_path_buf(),
            message: format!("Path item '{}' is not an object", path_template),
        })?;

        if let Some(operation_value) = path_obj.get(method) {
            let resolved_operation = resolve_reference(operation_value, spec, spec_path)?;
            return build_operation_match(
                path_template,
                resolved_path.clone(),
                resolved_operation.clone(),
                spec,
                spec_path,
            );
        }
    }

    Err(Error::OpenApi {
        path: spec_path.to_path_buf(),
        message: format!(
            "no OpenAPI operation for '{}' {}",
            method.to_uppercase(),
            route_path
        ),
    })
}

fn build_operation_match(
    path_template: &str,
    path_item: Value,
    operation_value: Value,
    spec: &Value,
    spec_path: &Path,
) -> Result<OperationMatch> {
    let info = extract_request_body_info(&operation_value, spec, spec_path)?;
    let parameters = collect_parameters(&path_item, &operation_value, spec, spec_path)?;
    let responses = extract_response_schemas(&operation_value, spec, spec_path)?;
    Ok(OperationMatch {
        path: path_template.to_string(),
        body_required: info.body_required,
        schema: info.schema,
        parameters,
        responses,
    })
}

struct RequestBodyInfo {
    schema: Option<Value>,
    body_required: bool,
}

fn extract_request_body_info(
    operation: &Value,
    spec: &Value,
    spec_path: &Path,
) -> Result<RequestBodyInfo> {
    let op_obj = operation.as_object().ok_or_else(|| Error::OpenApi {
        path: spec_path.to_path_buf(),
        message: "operation is not an object".to_string(),
    })?;

    let request_body = match op_obj.get("requestBody") {
        Some(body) => resolve_reference(body, spec, spec_path)?,
        None => {
            return Ok(RequestBodyInfo {
                schema: None,
                body_required: false,
            })
        }
    };

    let body_obj = request_body.as_object().ok_or_else(|| Error::OpenApi {
        path: spec_path.to_path_buf(),
        message: "requestBody must be an object".to_string(),
    })?;

    let body_required = body_obj
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let content = match body_obj.get("content").and_then(Value::as_object) {
        Some(map) => map,
        None => {
            return Ok(RequestBodyInfo {
                schema: None,
                body_required,
            })
        }
    };

    let media_type = match select_json_media_type(content) {
        Some(media) => media,
        None => {
            return Ok(RequestBodyInfo {
                schema: None,
                body_required,
            })
        }
    };

    let media_obj = media_type.as_object().ok_or_else(|| Error::OpenApi {
        path: spec_path.to_path_buf(),
        message: "media type must be an object".to_string(),
    })?;

    let schema = match media_obj.get("schema") {
        Some(schema_val) => resolve_reference(schema_val, spec, spec_path)?,
        None => {
            return Ok(RequestBodyInfo {
                schema: None,
                body_required,
            })
        }
    };

    Ok(RequestBodyInfo {
        schema: Some(schema.clone()),
        body_required,
    })
}

fn collect_parameters(
    path_item: &Value,
    operation: &Value,
    spec: &Value,
    spec_path: &Path,
) -> Result<Vec<ParameterSpec>> {
    let mut params = Vec::new();

    if let Some(list) = path_item.get("parameters").and_then(Value::as_array) {
        for param in list {
            if let Some(parsed) = parse_parameter(param, spec, spec_path)? {
                upsert_parameter(&mut params, parsed);
            }
        }
    }

    if let Some(list) = operation.get("parameters").and_then(Value::as_array) {
        for param in list {
            if let Some(parsed) = parse_parameter(param, spec, spec_path)? {
                upsert_parameter(&mut params, parsed);
            }
        }
    }

    Ok(params)
}

fn parse_parameter(value: &Value, spec: &Value, spec_path: &Path) -> Result<Option<ParameterSpec>> {
    let resolved = resolve_reference(value, spec, spec_path)?;
    let obj = resolved.as_object().ok_or_else(|| Error::OpenApi {
        path: spec_path.to_path_buf(),
        message: "parameter must be an object".to_string(),
    })?;

    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::OpenApi {
            path: spec_path.to_path_buf(),
            message: "parameter missing 'name'".to_string(),
        })?
        .to_string();

    let location = match obj.get("in").and_then(Value::as_str) {
        Some("path") => ParameterLocation::Path,
        Some("query") => ParameterLocation::Query,
        Some("header") => ParameterLocation::Header,
        Some("cookie") => ParameterLocation::Cookie,
        Some(other) => {
            tracing::warn!("Unsupported OpenAPI parameter location '{}'", other);
            return Ok(None);
        }
        None => {
            return Err(Error::OpenApi {
                path: spec_path.to_path_buf(),
                message: format!("parameter '{}' missing 'in'", name),
            })
        }
    };

    let mut required = obj
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if matches!(location, ParameterLocation::Path) {
        required = true;
    }

    let schema = match obj.get("schema") {
        Some(schema_value) => Some(resolve_schema_value(schema_value, spec, spec_path)?),
        None => None,
    };

    Ok(Some(ParameterSpec {
        name,
        location,
        required,
        schema,
    }))
}

fn upsert_parameter(params: &mut Vec<ParameterSpec>, spec: ParameterSpec) {
    if let Some(existing) = params
        .iter_mut()
        .find(|p| p.name == spec.name && p.location == spec.location)
    {
        *existing = spec;
    } else {
        params.push(spec);
    }
}

fn extract_response_schemas(
    operation: &Value,
    spec: &Value,
    spec_path: &Path,
) -> Result<HashMap<ResponseKey, Value>> {
    let mut map = HashMap::new();
    let responses = match operation.get("responses").and_then(Value::as_object) {
        Some(map) => map,
        None => return Ok(map),
    };

    for (status_key, response_value) in responses {
        let resolved_response = resolve_reference(response_value, spec, spec_path)?;
        let content = match resolved_response.get("content").and_then(Value::as_object) {
            Some(content) => content,
            None => continue,
        };

        let media = match select_json_media_type(content) {
            Some(media) => media,
            None => continue,
        };

        let media_obj = media.as_object().ok_or_else(|| Error::OpenApi {
            path: spec_path.to_path_buf(),
            message: "response media type must be an object".to_string(),
        })?;

        let schema_value = match media_obj.get("schema") {
            Some(schema) => resolve_schema_value(schema, spec, spec_path)?,
            None => continue,
        };

        if let Some(key) = parse_response_key(status_key) {
            map.insert(key, schema_value);
        }
    }

    Ok(map)
}

fn parse_response_key(raw: &str) -> Option<ResponseKey> {
    if raw.eq_ignore_ascii_case("default") {
        return Some(ResponseKey::Default);
    }
    raw.parse::<u16>().ok().map(ResponseKey::Status)
}

fn compile_parameter_validators(
    specs: Vec<ParameterSpec>,
    spec_path: &Path,
) -> Result<Vec<ParameterValidator>> {
    let mut validators = Vec::new();
    for spec in specs {
        let primitive = spec.schema.as_ref().and_then(detect_primitive_type);
        let schema_arc = match spec.schema {
            Some(schema_value) => Some(Arc::new(JSONSchema::compile(&schema_value).map_err(
                |e| Error::InvalidOpenApi {
                    path: spec_path.to_path_buf(),
                    message: e.to_string(),
                },
            )?)),
            None => None,
        };

        validators.push(ParameterValidator {
            name: spec.name,
            location: spec.location,
            required: spec.required,
            schema: schema_arc,
            value_type: primitive,
        });
    }
    Ok(validators)
}

fn compile_response_schemas(
    responses: HashMap<ResponseKey, Value>,
    spec_path: &Path,
) -> Result<HashMap<ResponseKey, Arc<JSONSchema>>> {
    let mut compiled = HashMap::new();
    for (key, schema_value) in responses {
        let schema =
            Arc::new(
                JSONSchema::compile(&schema_value).map_err(|e| Error::InvalidOpenApi {
                    path: spec_path.to_path_buf(),
                    message: e.to_string(),
                })?,
            );
        compiled.insert(key, schema);
    }
    Ok(compiled)
}

fn detect_primitive_type(schema: &Value) -> Option<PrimitiveType> {
    match schema.get("type").and_then(Value::as_str) {
        Some("integer") => Some(PrimitiveType::Integer),
        Some("number") => Some(PrimitiveType::Number),
        Some("boolean") => Some(PrimitiveType::Boolean),
        _ => None,
    }
}

fn resolve_reference<'a>(value: &'a Value, spec: &'a Value, spec_path: &Path) -> Result<&'a Value> {
    if let Some(obj) = value.as_object() {
        if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
            let pointer = reference.strip_prefix('#').ok_or_else(|| Error::OpenApi {
                path: spec_path.to_path_buf(),
                message: format!("unsupported external reference '{}'", reference),
            })?;

            return spec.pointer(pointer).ok_or_else(|| Error::OpenApi {
                path: spec_path.to_path_buf(),
                message: format!("reference '{}' not found", reference),
            });
        }
    }

    Ok(value)
}

fn select_json_media_type(content: &Map<String, Value>) -> Option<&Value> {
    const PREFERRED: [&str; 2] = ["application/json", "application/*+json"];
    for key in PREFERRED {
        if let Some(media) = content.get(key) {
            return Some(media);
        }
    }

    content.iter().find_map(|(k, v)| {
        if k.to_ascii_lowercase().contains("json") {
            Some(v)
        } else {
            None
        }
    })
}

fn paths_match(route_path: &str, spec_path: &str) -> bool {
    let route_segments = split_path(route_path);
    let spec_segments = split_path(spec_path);

    if route_segments.len() != spec_segments.len() {
        return false;
    }

    for (route_seg, spec_seg) in route_segments.iter().zip(spec_segments.iter()) {
        let route_is_param = is_route_param(route_seg);
        let spec_is_param = is_spec_param(spec_seg);

        if route_is_param || spec_is_param {
            continue;
        }

        if route_seg != spec_seg {
            return false;
        }
    }

    true
}

fn split_path(path: &str) -> Vec<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Vec::new();
    }

    trimmed.split('/').map(|s| s.to_string()).collect()
}

fn is_route_param(segment: &str) -> bool {
    segment.starts_with(':') || (segment.starts_with('{') && segment.ends_with('}'))
}

fn is_spec_param(segment: &str) -> bool {
    segment.starts_with('{') && segment.ends_with('}')
}

fn resolve_schema_value(schema: &Value, spec: &Value, spec_path: &Path) -> Result<Value> {
    if let Some(obj) = schema.as_object() {
        if obj.contains_key("$ref") {
            let resolved = resolve_reference(schema, spec, spec_path)?;
            return resolve_schema_value(resolved, spec, spec_path);
        }
    }

    match schema {
        Value::Object(map) => {
            let mut resolved = Map::new();
            for (key, value) in map {
                resolved.insert(key.clone(), resolve_schema_value(value, spec, spec_path)?);
            }
            Ok(Value::Object(resolved))
        }
        Value::Array(items) => {
            let mut resolved_items = Vec::new();
            for item in items {
                resolved_items.push(resolve_schema_value(item, spec, spec_path)?);
            }
            Ok(Value::Array(resolved_items))
        }
        _ => Ok(schema.clone()),
    }
}

#[derive(Hash, Eq, PartialEq, Clone)]
struct OperationCacheKey {
    spec_path: PathBuf,
    method: String,
    path_template: String,
}

const METHOD_KEYS: &[&str] = &[
    "get", "put", "post", "delete", "options", "head", "patch", "trace",
];
