use thiserror::Error;
use std::path::PathBuf;

#[derive(Debug, Error)]
pub enum EntityInspectorError {
    #[error("File not found: {path}")]
    FileNotFound { path: PathBuf },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Template {id} not found")]
    TemplateNotFound { id: i32 },

    #[error("Entity with name '{name}' not found")]
    EntityNotFound { name: String },

    #[error("Property parsing failed: {message}")]
    PropertyParsingError { message: String },

    #[error("Invalid template range format: {range}")]
    InvalidTemplateRange { range: String },

    #[error("Export error: {message}")]
    ExportError { message: String },

    #[error("Validation error: {message}")]
    ValidationError { message: String },

    #[error("Parse error: {message}")]
    ParseError { message: String },
}

pub type Result<T> = std::result::Result<T, EntityInspectorError>;