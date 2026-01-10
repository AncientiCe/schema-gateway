use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use jsonschema::JSONSchema;
use serde_json::Value;

pub struct SchemaCache {
    pub cache: HashMap<PathBuf, Arc<JSONSchema>>,
}

impl SchemaCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn load<P: AsRef<Path>>(&mut self, path: P) -> Result<Arc<JSONSchema>> {
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

        let compiled = JSONSchema::compile(&value).map_err(|e| Error::InvalidSchemaSyntax {
            path: path_buf.clone(),
            message: e.to_string(),
        })?;

        let arc = Arc::new(compiled);
        self.cache.insert(path_buf, Arc::clone(&arc));
        Ok(arc)
    }
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new()
    }
}
