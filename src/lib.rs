pub mod analysis;
pub mod cli;
pub mod client_generator;
pub mod config;
pub mod error;
pub mod generator;
pub mod http_config;
pub mod http_error;
pub mod openapi;
pub mod patterns;
pub mod streaming;

pub mod test_helpers;

pub use analysis::{SchemaAnalysis, SchemaAnalyzer, merge_schema_extensions};
pub use config::ConfigFile;
pub use error::GeneratorError;
pub use generator::{CodeGenerator, GeneratedFile, GenerationResult, GeneratorConfig};
pub use http_config::{AuthConfig, HttpClientConfig, RetryConfig};
pub use http_error::{HttpError, HttpResult};
pub use openapi::{OpenApiSpec, Schema, SchemaType};

pub type Result<T> = std::result::Result<T, GeneratorError>;
