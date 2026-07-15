pub mod analysis;
pub mod cli;
pub mod client_generator;
pub mod config;
pub mod error;
pub mod extensions;
pub mod generator;
pub mod http_config;
pub mod http_error;
pub mod openapi;
pub mod patterns;
pub mod registry_generator;
pub mod server;
pub mod streaming;
pub mod type_mapping;

/// Helpers for generator snapshot and scratch-crate tests.
///
/// This module is excluded from default builds so its test-only dependencies
/// do not become part of the installed CLI. Enable the `test-helpers` feature
/// when using these helpers outside this repository's test suite.
#[cfg(feature = "test-helpers")]
pub mod test_helpers;

pub use analysis::{SchemaAnalysis, SchemaAnalyzer, merge_schema_extensions};
pub use config::ConfigFile;
pub use error::GeneratorError;
pub use generator::{CodeGenerator, GeneratedFile, GenerationResult, GeneratorConfig};
pub use http_config::{AuthConfig, HttpClientConfig, RetryConfig};
pub use http_error::{ApiError, ApiOpError, HttpError, HttpResult};
pub use openapi::{OpenApiSpec, Schema, SchemaType};
pub use type_mapping::{
    ByteStrategy, DepRequirement, MappedType, TypeFeature, TypeMapper, TypeMappingConfig,
    UsedFeatures,
};

pub type Result<T> = std::result::Result<T, GeneratorError>;
