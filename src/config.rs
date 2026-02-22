use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub routes: Vec<Route>,
    pub global: GlobalConfig,
    // Route index for O(1) lookups - built after deserialization
    route_index: RouteIndex,
}

// Custom deserialization to build index after parsing
impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ConfigHelper {
            routes: Vec<Route>,
            #[serde(default)]
            global: GlobalConfig,
        }

        let helper = ConfigHelper::deserialize(deserializer)?;
        let mut config = Config {
            routes: helper.routes,
            global: helper.global,
            route_index: RouteIndex::new(),
        };
        config.build_route_index();
        Ok(config)
    }
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path_ref = path.as_ref();

        // Read file contents
        let contents = fs::read_to_string(path_ref)
            .map_err(|e| format!("Failed to read config file '{}': {}", path_ref.display(), e))?;

        // Parse YAML
        let mut config: Config = serde_yaml::from_str(&contents).map_err(|e| {
            format!(
                "Failed to parse config file '{}': {}",
                path_ref.display(),
                e
            )
        })?;

        // Build route index for fast lookups
        config.build_route_index();

        Ok(config)
    }

    /// Build the route index for O(1) lookups
    fn build_route_index(&mut self) {
        self.route_index = RouteIndex::new();
        for (idx, route) in self.routes.iter().enumerate() {
            let method = route.method.to_uppercase();
            self.route_index.add_route(method, route.path.clone(), idx);
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        // Check for empty routes
        if self.routes.is_empty() {
            return Err("Config must have at least one route".to_string());
        }

        // Validate each route
        for (idx, route) in self.routes.iter().enumerate() {
            if let Err(e) = route.validate() {
                return Err(format!("Route {}: {}", idx, e));
            }
        }

        Ok(())
    }

    pub fn find_route(&self, path: &str, method: &str) -> Option<&Route> {
        // Use indexed lookup for O(1) exact matches, O(n) for parameterized routes
        if let Some(route_idx) = self.route_index.find_route(path, method) {
            return self.routes.get(route_idx);
        }
        // Fallback to linear search (shouldn't happen if index was built correctly)
        self.routes.iter().find(|route| route.matches(path, method))
    }

    pub fn get_effective_config(&self, route: &Route) -> GlobalConfig {
        GlobalConfig {
            forward_on_error: route
                .config
                .forward_on_error
                .unwrap_or(self.global.forward_on_error),
            add_error_header: route
                .config
                .add_error_header
                .unwrap_or(self.global.add_error_header),
            add_validation_header: route
                .config
                .add_validation_header
                .unwrap_or(self.global.add_validation_header),
        }
    }

    /// Returns all file paths that should be watched for hot-reload: the config file itself,
    /// plus every referenced schema and OpenAPI spec path. Relative paths are resolved
    /// against the config file's parent directory.
    pub fn watched_paths(&self, config_file: &Path) -> Vec<PathBuf> {
        let config_dir = config_file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));

        let mut paths = vec![config_file.to_path_buf()];

        for route in &self.routes {
            if let Some(ref schema_path) = route.schema {
                let resolved = if schema_path.is_relative() {
                    config_dir.join(schema_path)
                } else {
                    schema_path.clone()
                };
                paths.push(resolved);
            }
            if let Some(ref openapi_source) = route.openapi {
                let spec_path = openapi_source.to_options().spec;
                let resolved = if spec_path.is_relative() {
                    config_dir.join(spec_path)
                } else {
                    spec_path
                };
                paths.push(resolved);
            }
        }

        paths
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub schema: Option<PathBuf>,
    #[serde(default)]
    pub openapi: Option<OpenApiSource>,
    pub upstream: String,
    #[serde(default)]
    pub config: RouteConfig,
}

impl Route {
    fn validate(&self) -> Result<(), String> {
        // Check for empty upstream
        if self.upstream.is_empty() {
            return Err("upstream cannot be empty".to_string());
        }

        // Check for valid HTTP method
        let valid_methods = [
            "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "CONNECT", "TRACE",
        ];
        let method_upper = self.method.to_uppercase();
        if !valid_methods.contains(&method_upper.as_str()) {
            return Err(format!("Invalid HTTP method: {}", self.method));
        }

        if self.schema.is_some() && self.openapi.is_some() {
            return Err("Cannot specify both 'schema' and 'openapi' on a route".to_string());
        }

        if let Some(openapi) = self.openapi.as_ref().map(OpenApiSource::to_options) {
            if openapi.spec.as_os_str().is_empty() {
                return Err("OpenAPI spec path cannot be empty".to_string());
            }
            if !openapi.spec.exists() {
                return Err(format!(
                    "OpenAPI spec does not exist: {}",
                    openapi.spec.display()
                ));
            }
            if let Some(op_id) = openapi.operation_id.as_ref() {
                if op_id.trim().is_empty() {
                    return Err("OpenAPI operation_id cannot be empty".to_string());
                }
            }
        }

        Ok(())
    }

    pub fn matches(&self, path: &str, method: &str) -> bool {
        // Case-insensitive method matching
        if self.method.to_uppercase() != method.to_uppercase() {
            return false;
        }

        // Split paths by '/' and compare segments
        let route_segments: Vec<&str> = self.path.split('/').collect();
        let path_segments: Vec<&str> = path.split('/').collect();

        // Must have same number of segments
        if route_segments.len() != path_segments.len() {
            return false;
        }

        // Compare each segment
        for (route_seg, path_seg) in route_segments.iter().zip(path_segments.iter()) {
            // Segments starting with ':' are wildcards (path parameters)
            if route_seg.starts_with(':') {
                continue;
            }

            // Static segments must match exactly
            if route_seg != path_seg {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct GlobalConfig {
    #[serde(default = "default_true")]
    pub forward_on_error: bool,
    #[serde(default = "default_true")]
    pub add_error_header: bool,
    #[serde(default = "default_true")]
    pub add_validation_header: bool,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            forward_on_error: true,
            add_error_header: true,
            add_validation_header: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Default)]
pub struct RouteConfig {
    pub forward_on_error: Option<bool>,
    pub add_error_header: Option<bool>,
    pub add_validation_header: Option<bool>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum OpenApiSource {
    Spec(PathBuf),
    Detailed(OpenApiRouteConfig),
}

impl OpenApiSource {
    pub fn to_options(&self) -> OpenApiOptions {
        match self {
            OpenApiSource::Spec(path) => OpenApiOptions {
                spec: path.clone(),
                operation_id: None,
            },
            OpenApiSource::Detailed(cfg) => OpenApiOptions {
                spec: cfg.spec.clone(),
                operation_id: cfg.operation_id.clone().filter(|s| !s.trim().is_empty()),
            },
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct OpenApiRouteConfig {
    pub spec: PathBuf,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApiOptions {
    pub spec: PathBuf,
    pub operation_id: Option<String>,
}

impl Route {
    pub fn openapi_options(&self) -> Option<OpenApiOptions> {
        self.openapi.as_ref().map(OpenApiSource::to_options)
    }
}

/// Route index for fast O(1) route lookups by method and path pattern
#[derive(Debug, Clone)]
struct RouteIndex {
    // Index by method -> path pattern -> route index
    by_method: HashMap<String, HashMap<String, usize>>,
    // For routes with parameters, we need to check them separately
    // Store routes with parameters separately for pattern matching
    param_routes: HashMap<String, Vec<(String, usize)>>,
}

impl RouteIndex {
    fn new() -> Self {
        Self {
            by_method: HashMap::new(),
            param_routes: HashMap::new(),
        }
    }

    fn add_route(&mut self, method: String, path_pattern: String, route_idx: usize) {
        // Check if path has parameters
        let has_params = path_pattern.contains(':');

        if has_params {
            // Store in param_routes for pattern matching
            self.param_routes
                .entry(method.clone())
                .or_default()
                .push((path_pattern, route_idx));
        } else {
            // Exact match - can use HashMap for O(1) lookup
            self.by_method
                .entry(method)
                .or_default()
                .insert(path_pattern, route_idx);
        }
    }

    fn find_route(&self, path: &str, method: &str) -> Option<usize> {
        let method_upper = method.to_uppercase();

        // First, try exact match (no parameters)
        if let Some(method_routes) = self.by_method.get(&method_upper) {
            if let Some(&route_idx) = method_routes.get(path) {
                return Some(route_idx);
            }
        }

        // If not found, check parameterized routes
        if let Some(param_routes) = self.param_routes.get(&method_upper) {
            for (pattern, route_idx) in param_routes {
                // Use the existing Route::matches logic
                // We need access to the actual route to call matches, so we'll
                // do a quick pattern match here
                if paths_match(path, pattern) {
                    return Some(*route_idx);
                }
            }
        }

        None
    }
}

/// Check if a path matches a pattern (supports :param placeholders)
fn paths_match(path: &str, pattern: &str) -> bool {
    let path_segments: Vec<&str> = path.split('/').collect();
    let pattern_segments: Vec<&str> = pattern.split('/').collect();

    if path_segments.len() != pattern_segments.len() {
        return false;
    }

    for (path_seg, pattern_seg) in path_segments.iter().zip(pattern_segments.iter()) {
        // Segments starting with ':' are wildcards (path parameters)
        if pattern_seg.starts_with(':') {
            continue;
        }

        // Static segments must match exactly
        if path_seg != pattern_seg {
            return false;
        }
    }

    true
}
