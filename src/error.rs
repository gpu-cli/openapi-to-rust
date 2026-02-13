use thiserror::Error;

#[derive(Error, Debug)]
pub enum GeneratorError {
    #[error("Failed to parse OpenAPI spec: {0}")]
    ParseError(#[from] serde_json::Error),

    #[error("Unresolved schema reference: {0}")]
    UnresolvedReference(String),

    #[error("Circular dependency detected: {0}")]
    CircularDependency(String),

    #[error("Invalid discriminator pattern in schema: {0}")]
    InvalidDiscriminator(String),

    #[error("Code generation failed: {0}")]
    CodeGenError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid schema: {0}")]
    InvalidSchema(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("File error: {message}")]
    FileError { message: String },
}

impl GeneratorError {
    pub fn with_context(self, schema_name: &str) -> Self {
        match self {
            Self::CodeGenError(msg) => {
                Self::CodeGenError(format!("{msg} (in schema: {schema_name})"))
            }
            Self::InvalidSchema(msg) => {
                Self::InvalidSchema(format!("{msg} (in schema: {schema_name})"))
            }
            other => other,
        }
    }
}
