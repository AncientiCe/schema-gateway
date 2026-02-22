use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use jsonschema::Validator;
use serde_json::Value;

pub struct SchemaCache {
    pub cache: HashMap<PathBuf, Arc<Validator>>,
}

impl SchemaCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Get schema from cache (read-only, no lock needed if called on &self)
    pub fn get<P: AsRef<Path>>(&self, path: P) -> Option<Arc<Validator>> {
        let path_buf = PathBuf::from(path.as_ref());
        self.cache.get(&path_buf).map(Arc::clone)
    }

    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<Arc<Validator>> {
        let path_ref = path.as_ref();
        let path_buf = PathBuf::from(path_ref);

        if let Some(schema) = self.cache.get(&path_buf) {
            return Ok(Arc::clone(schema));
        }

        let contents = match fs::read_to_string(&path_buf) {
            Ok(s) => s,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Err(Error::SchemaNotFound { path: path_buf });
                }
                return Err(Error::Io(e));
            }
        };

        let value: Value =
            serde_json::from_str(&contents).map_err(|e| Error::InvalidSchemaJson {
                path: path_buf.clone(),
                source: e,
            })?;

        let compiled =
            jsonschema::validator_for(&value).map_err(|e| Error::InvalidSchemaSyntax {
                path: path_buf.clone(),
                message: e.to_string(),
            })?;

        let arc = Arc::new(compiled);
        self.cache.insert(path_buf, Arc::clone(&arc));
        Ok(arc)
    }

    /// Preload all schemas from the given paths
    /// Returns a vector of errors for any schemas that failed to load
    pub fn preload_all<P: AsRef<Path>, I: IntoIterator<Item = P>>(
        &mut self,
        paths: I,
    ) -> Vec<(PathBuf, String)> {
        let mut errors = Vec::new();
        for path in paths {
            let path_buf = PathBuf::from(path.as_ref());
            if self.cache.contains_key(&path_buf) {
                // Already loaded, skip
                continue;
            }
            match self.load(&path_buf) {
                Ok(_) => {
                    tracing::debug!("Preloaded schema: {}", path_buf.display());
                }
                Err(e) => {
                    let error_msg = format!("{}", e);
                    errors.push((path_buf, error_msg));
                    tracing::warn!(
                        "Failed to preload schema {}: {}",
                        path.as_ref().display(),
                        e
                    );
                }
            }
        }
        errors
    }
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new()
    }
}
