use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Config {
    pub routes: Vec<Route>,
    #[serde(default)]
    pub global: GlobalConfig,
}

impl Config {
    /// Load configuration from a YAML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path_ref = path.as_ref();

        // Read file contents
        let contents = fs::read_to_string(path_ref)
            .map_err(|e| format!("Failed to read config file '{}': {}", path_ref.display(), e))?;

        // Parse YAML
        let config: Config = serde_yaml::from_str(&contents).map_err(|e| {
            format!(
                "Failed to parse config file '{}': {}",
                path_ref.display(),
                e
            )
        })?;

        Ok(config)
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
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct Route {
    pub path: String,
    pub method: String,
    pub schema: Option<PathBuf>,
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
