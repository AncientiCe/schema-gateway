use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Schema not found: {path}")]
    SchemaNotFound { path: PathBuf },

    #[error("Invalid schema JSON in {path}: {source}")]
    InvalidSchemaJson {
        path: PathBuf,
        source: serde_json::Error,
    },

    #[error("Invalid schema syntax in {path}: {message}")]
    InvalidSchemaSyntax { path: PathBuf, message: String },

    #[error(transparent)]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
