//! Generate typed Rust models, async HTTP/SSE clients, and opt-in Axum server
//! scaffolding from OpenAPI 3.0 and 3.1 documents (with experimental 3.2
//! parsing).
//!
//! Most users should install the `openapi-to-rust` CLI and start with
//! `openapi-to-rust generate <SOURCE>`. The library API is useful when a build
//! tool or application needs to analyze and render a document in memory.
//!
//! # Library example
//!
//! ```
//! use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
//! use serde_json::json;
//!
//! # fn main() -> openapi_to_rust::Result<()> {
//! let spec = json!({
//!     "openapi": "3.1.0",
//!     "info": { "title": "Example", "version": "1.0.0" },
//!     "paths": {},
//!     "components": {
//!         "schemas": {
//!             "Greeting": {
//!                 "type": "object",
//!                 "required": ["message"],
//!                 "properties": { "message": { "type": "string" } }
//!             }
//!         }
//!     }
//! });
//!
//! let mut analyzer = SchemaAnalyzer::new(spec)?;
//! let mut analysis = analyzer.analyze()?;
//! let generator = CodeGenerator::new(GeneratorConfig {
//!     enable_async_client: false,
//!     ..GeneratorConfig::default()
//! });
//! let source = generator.generate(&mut analysis)?;
//! assert!(source.contains("pub struct Greeting"));
//! # Ok(())
//! # }
//! ```
//!
//! Configuration is documented in [`config`]. Generated dependency fragments
//! are represented by [`DepRequirement`], and generated files can be written
//! with [`CodeGenerator::write_files`].

pub mod analysis;
#[cfg(feature = "cli")]
pub mod cli;
pub mod client_generator;
pub mod config;
pub mod error;
pub mod extensions;
pub mod generator;
pub mod http_config;
#[cfg(feature = "http-error")]
pub mod http_error;
pub mod openapi;
pub mod patterns;
pub mod registry_generator;
pub mod server;
pub mod spec_source;
pub mod streaming;
pub mod type_mapping;

/// Helpers for generator snapshot and scratch-crate tests.
///
/// This module is excluded from default builds so its test-only dependencies
/// do not become part of the installed CLI. Enable the `test-helpers` feature
/// when using these helpers outside this repository's test suite.
#[cfg(feature = "test-helpers")]
pub mod test_helpers;

/// Crate version, exposed so embedders (e.g. the WASM playground) can report
/// which generator produced their output.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use analysis::{SchemaAnalysis, SchemaAnalyzer, merge_schema_extensions};
pub use config::ConfigFile;
pub use error::GeneratorError;
pub use generator::{CodeGenerator, GeneratedFile, GenerationResult, GeneratorConfig};
pub use http_config::{AuthConfig, HttpClientConfig, RetryConfig};
#[cfg(feature = "http-error")]
pub use http_error::{ApiError, ApiOpError, HttpError, HttpResult};
pub use openapi::{OpenApiSpec, Schema, SchemaType};
pub use type_mapping::{
    ByteStrategy, DepRequirement, MappedType, TypeFeature, TypeMapper, TypeMappingConfig,
    UsedFeatures,
};

pub type Result<T> = std::result::Result<T, GeneratorError>;
