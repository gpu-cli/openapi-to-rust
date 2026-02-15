//! TOML configuration file support for OpenAPI code generation.
//!
//! This module provides TOML-based configuration as an alternative to the Rust API.
//! It enables CLI-based code generation without requiring the generator as a build dependency.
//!
//! # Overview
//!
//! The TOML configuration system provides:
//! - Declarative configuration in `openapi-to-rust.toml` files
//! - Comprehensive validation with helpful error messages
//! - Support for all generator features (HTTP client, retry, tracing, Specta)
//! - Conversion to internal [`GeneratorConfig`] for code generation
//!
//! # Quick Start
//!
//! Create an `openapi-to-rust.toml` file:
//!
//! ```toml
//! [generator]
//! spec_path = "openapi.json"
//! output_dir = "src/generated"
//! module_name = "api"
//!
//! [features]
//! enable_async_client = true
//!
//! [http_client]
//! base_url = "https://api.example.com"
//! timeout_seconds = 30
//!
//! [http_client.retry]
//! max_retries = 3
//! initial_delay_ms = 500
//! max_delay_ms = 16000
//! ```
//!
//! Load and use the configuration:
//!
//! ```no_run
//! use openapi_to_rust::config::ConfigFile;
//! use std::path::Path;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load configuration from TOML file
//! let config_file = ConfigFile::load(Path::new("openapi-to-rust.toml"))?;
//!
//! // Convert to internal GeneratorConfig
//! let generator_config = config_file.into_generator_config();
//!
//! // Use with CodeGenerator...
//! # Ok(())
//! # }
//! ```
//!
//! # Configuration Sections
//!
//! ## Generator Section (Required)
//!
//! ```toml
//! [generator]
//! spec_path = "openapi.json"       # Path to OpenAPI spec
//! output_dir = "src/generated"     # Output directory
//! module_name = "api"              # Module name
//! ```
//!
//! ## Features Section (Optional)
//!
//! ```toml
//! [features]
//! enable_sse_client = true         # Generate SSE streaming client
//! enable_async_client = true       # Generate HTTP REST client
//! enable_specta = false            # Add specta::Type derives
//! ```
//!
//! ## HTTP Client Section (Optional)
//!
//! ```toml
//! [http_client]
//! base_url = "https://api.example.com"
//! timeout_seconds = 30
//!
//! [http_client.retry]
//! max_retries = 3                  # 0-10 retries
//! initial_delay_ms = 500           # 100-10000ms
//! max_delay_ms = 16000             # 1000-300000ms
//!
//! [http_client.tracing]
//! enabled = true                   # Enable request tracing (default: true)
//!
//! [http_client.auth]
//! type = "Bearer"                  # Bearer, ApiKey, or Custom
//! header_name = "Authorization"
//!
//! [[http_client.headers]]
//! name = "content-type"
//! value = "application/json"
//! ```
//!
//! # Validation
//!
//! The configuration is validated on load using the `validator` crate:
//! - File paths are checked for existence
//! - Numeric ranges are enforced (timeout, retry counts, delays)
//! - Enum values are validated (auth types, event flow types)
//! - Required fields are checked
//!
//! Invalid configurations produce helpful error messages:
//!
//! ```text
//! Configuration validation failed:
//!   - generator.spec_path: OpenAPI spec file not found: missing.json
//!   - http_client.retry.max_retries: max_retries must be between 0 and 10
//! ```
//!
//! # Examples
//!
//! See the [examples](https://github.com/your-repo/examples) directory for complete examples:
//! - `toml_config_example.rs` - Various configuration patterns
//! - `complete_workflow.rs` - Full generation workflow with TOML
//!
//! # Backward Compatibility
//!
//! The TOML configuration is fully optional. The existing Rust API continues to work:
//!
//! ```no_run
//! use openapi_to_rust::{GeneratorConfig, CodeGenerator};
//! use std::path::PathBuf;
//!
//! let config = GeneratorConfig {
//!     spec_path: PathBuf::from("openapi.json"),
//!     enable_async_client: true,
//!     // ... other fields
//!     ..Default::default()
//! };
//!
//! let generator = CodeGenerator::new(config);
//! // ... generate code
//! ```

use crate::{GeneratorError, generator::GeneratorConfig};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use validator::Validate;

/// Root configuration loaded from TOML file
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct ConfigFile {
    #[validate(nested)]
    pub generator: GeneratorSection,
    #[validate(nested)]
    pub features: FeaturesSection,
    #[serde(default)]
    #[validate(nested)]
    pub http_client: Option<HttpClientSection>,
    #[serde(default)]
    #[validate(nested)]
    pub streaming: Option<StreamingSection>,
    #[serde(default)]
    pub nullable_overrides: BTreeMap<String, bool>,
    #[serde(default)]
    pub type_mappings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct GeneratorSection {
    #[validate(custom(function = "validate_spec_path_exists"))]
    pub spec_path: PathBuf,
    pub output_dir: PathBuf,
    #[validate(length(min = 1, message = "module_name cannot be empty"))]
    pub module_name: String,
    /// Schema extension files to merge into the main spec before codegen.
    /// Paths are relative to the working directory (same as spec_path).
    #[serde(default)]
    pub schema_extensions: Vec<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct FeaturesSection {
    #[serde(default)]
    pub enable_sse_client: bool,
    #[serde(default)]
    pub enable_async_client: bool,
    #[serde(default)]
    pub enable_specta: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct HttpClientSection {
    #[validate(url(message = "base_url must be a valid URL"))]
    pub base_url: Option<String>,
    #[validate(custom(function = "validate_timeout_seconds"))]
    pub timeout_seconds: Option<u64>,
    #[validate(nested)]
    pub auth: Option<AuthConfigSection>,
    #[serde(default)]
    #[validate(nested)]
    pub headers: Vec<HeaderEntry>,
    #[validate(nested)]
    pub retry: Option<RetryConfigSection>,
    #[validate(nested)]
    pub tracing: Option<TracingConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct TracingConfigSection {
    #[serde(default = "default_tracing_enabled")]
    pub enabled: bool,
}

fn default_tracing_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct RetryConfigSection {
    #[serde(default = "default_max_retries")]
    #[validate(custom(function = "validate_max_retries"))]
    pub max_retries: u32,
    #[serde(default = "default_initial_delay_ms")]
    #[validate(custom(function = "validate_initial_delay_ms"))]
    pub initial_delay_ms: u64,
    #[serde(default = "default_max_delay_ms")]
    #[validate(custom(function = "validate_max_delay_ms"))]
    pub max_delay_ms: u64,
}

fn default_max_retries() -> u32 {
    3
}
fn default_initial_delay_ms() -> u64 {
    500
}
fn default_max_delay_ms() -> u64 {
    16000
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct AuthConfigSection {
    #[serde(rename = "type")]
    #[validate(custom(function = "validate_auth_type"))]
    pub auth_type: String,
    #[validate(length(min = 1, message = "header_name cannot be empty"))]
    pub header_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct HeaderEntry {
    #[validate(length(min = 1, message = "header name cannot be empty"))]
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct StreamingSection {
    #[validate(nested)]
    pub endpoints: Vec<StreamingEndpointSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct StreamingEndpointSection {
    #[validate(length(min = 1))]
    pub operation_id: String,
    #[validate(length(min = 1))]
    pub path: String,
    /// HTTP method: "GET" or "POST" (default: POST)
    #[serde(default)]
    pub http_method: Option<String>,
    /// Parameter name that controls streaming (only for POST requests)
    #[serde(default)]
    pub stream_parameter: String,
    /// Query parameters for GET requests
    #[serde(default)]
    pub query_parameters: Vec<QueryParameterSection>,
    #[validate(length(min = 1))]
    pub event_union_type: String,
    pub content_type: Option<String>,
    #[validate(nested)]
    pub event_flow: Option<EventFlowSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct QueryParameterSection {
    #[validate(length(min = 1))]
    pub name: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct EventFlowSection {
    #[serde(rename = "type")]
    #[validate(custom(function = "validate_event_flow_type"))]
    pub flow_type: String,
    pub start_events: Option<Vec<String>>,
    pub delta_events: Option<Vec<String>>,
    pub stop_events: Option<Vec<String>>,
}

// Custom validators
fn validate_spec_path_exists(path: &Path) -> Result<(), validator::ValidationError> {
    if !path.exists() {
        let mut error = validator::ValidationError::new("file_not_found");
        error.message = Some(
            format!(
                "OpenAPI spec file not found: {}. Ensure spec_path points to a valid OpenAPI JSON or YAML file.",
                path.display()
            )
            .into(),
        );
        return Err(error);
    }
    Ok(())
}

fn validate_auth_type(auth_type: &str) -> Result<(), validator::ValidationError> {
    match auth_type {
        "Bearer" | "ApiKey" | "Custom" => Ok(()),
        _ => {
            let mut error = validator::ValidationError::new("invalid_auth_type");
            error.message = Some(
                format!(
                    "Invalid auth type '{}'. Must be one of: Bearer, ApiKey, Custom",
                    auth_type
                )
                .into(),
            );
            Err(error)
        }
    }
}

fn validate_event_flow_type(flow_type: &str) -> Result<(), validator::ValidationError> {
    match flow_type {
        "StartDeltaStop" | "Continuous" => Ok(()),
        _ => {
            let mut error = validator::ValidationError::new("invalid_event_flow_type");
            error.message = Some(
                format!(
                    "Invalid event flow type '{}'. Must be one of: StartDeltaStop, Continuous",
                    flow_type
                )
                .into(),
            );
            Err(error)
        }
    }
}

fn validate_timeout_seconds(timeout: u64) -> Result<(), validator::ValidationError> {
    if !(1..=3600).contains(&timeout) {
        let mut error = validator::ValidationError::new("out_of_range");
        error.message = Some("timeout_seconds must be between 1 and 3600".into());
        return Err(error);
    }
    Ok(())
}

fn validate_max_retries(retries: u32) -> Result<(), validator::ValidationError> {
    if retries > 10 {
        let mut error = validator::ValidationError::new("out_of_range");
        error.message = Some("max_retries must be between 0 and 10".into());
        return Err(error);
    }
    Ok(())
}

fn validate_initial_delay_ms(delay: u64) -> Result<(), validator::ValidationError> {
    if !(100..=10000).contains(&delay) {
        let mut error = validator::ValidationError::new("out_of_range");
        error.message = Some("initial_delay_ms must be between 100 and 10000".into());
        return Err(error);
    }
    Ok(())
}

fn validate_max_delay_ms(delay: u64) -> Result<(), validator::ValidationError> {
    if !(1000..=300000).contains(&delay) {
        let mut error = validator::ValidationError::new("out_of_range");
        error.message = Some("max_delay_ms must be between 1000 and 300000".into());
        return Err(error);
    }
    Ok(())
}

impl ConfigFile {
    /// Load and validate configuration from TOML file
    pub fn load(path: &Path) -> Result<Self, GeneratorError> {
        let content = std::fs::read_to_string(path).map_err(|e| GeneratorError::FileError {
            message: format!("Failed to read config file '{}': {}", path.display(), e),
        })?;

        let config: ConfigFile =
            toml::from_str(&content).map_err(|e| GeneratorError::FileError {
                message: format!(
                    "Failed to parse TOML config: {}\n\nExample config:\n{}",
                    e, EXAMPLE_CONFIG
                ),
            })?;

        // Validate the configuration
        config.validate().map_err(|e| {
            GeneratorError::ValidationError(format!(
                "Configuration validation failed:\n{}",
                format_validation_errors(&e)
            ))
        })?;

        Ok(config)
    }

    /// Convert to internal GeneratorConfig
    pub fn into_generator_config(self) -> GeneratorConfig {
        use crate::http_config::{AuthConfig, HttpClientConfig, RetryConfig};

        // Convert HTTP client config
        let http_client_config = self.http_client.as_ref().map(|http| HttpClientConfig {
            base_url: http.base_url.clone(),
            timeout_seconds: http.timeout_seconds,
            default_headers: http
                .headers
                .iter()
                .map(|h| (h.name.clone(), h.value.clone()))
                .collect(),
        });

        // Convert retry config
        let retry_config = self
            .http_client
            .as_ref()
            .and_then(|http| http.retry.as_ref())
            .map(|retry| RetryConfig {
                max_retries: retry.max_retries,
                initial_delay_ms: retry.initial_delay_ms,
                max_delay_ms: retry.max_delay_ms,
            });

        // Convert tracing config
        let tracing_enabled = self
            .http_client
            .as_ref()
            .and_then(|http| http.tracing.as_ref())
            .map(|tracing| tracing.enabled)
            .unwrap_or(true);

        // Convert auth config
        let auth_config = self
            .http_client
            .as_ref()
            .and_then(|http| http.auth.as_ref())
            .map(|auth| match auth.auth_type.as_str() {
                "Bearer" => AuthConfig::Bearer {
                    header_name: auth.header_name.clone(),
                },
                "ApiKey" => AuthConfig::ApiKey {
                    header_name: auth.header_name.clone(),
                },
                "Custom" => AuthConfig::Custom {
                    header_name: auth.header_name.clone(),
                    header_value_prefix: None,
                },
                _ => AuthConfig::Bearer {
                    header_name: "Authorization".to_string(),
                },
            });

        // Convert streaming section to StreamingConfig
        let streaming_config = self.streaming.map(|section| {
            use crate::streaming::{
                EventFlow, HttpMethod, QueryParameter, StreamingConfig, StreamingEndpoint,
            };

            let endpoints = section
                .endpoints
                .into_iter()
                .map(|e| {
                    let event_flow = e
                        .event_flow
                        .map(|ef| match ef.flow_type.as_str() {
                            "start_delta_stop" => EventFlow::StartDeltaStop {
                                start_events: ef.start_events.unwrap_or_default(),
                                delta_events: ef.delta_events.unwrap_or_default(),
                                stop_events: ef.stop_events.unwrap_or_default(),
                            },
                            _ => EventFlow::Simple,
                        })
                        .unwrap_or(EventFlow::Simple);

                    let http_method = e
                        .http_method
                        .map(|m| match m.to_uppercase().as_str() {
                            "GET" => HttpMethod::Get,
                            _ => HttpMethod::Post,
                        })
                        .unwrap_or(HttpMethod::Post);

                    let query_parameters = e
                        .query_parameters
                        .into_iter()
                        .map(|qp| QueryParameter {
                            name: qp.name,
                            required: qp.required,
                        })
                        .collect();

                    StreamingEndpoint {
                        operation_id: e.operation_id,
                        path: e.path,
                        http_method,
                        stream_parameter: e.stream_parameter,
                        query_parameters,
                        event_union_type: e.event_union_type,
                        content_type: e.content_type,
                        event_flow,
                        ..Default::default()
                    }
                })
                .collect();

            StreamingConfig {
                endpoints,
                ..Default::default()
            }
        });

        GeneratorConfig {
            spec_path: self.generator.spec_path,
            output_dir: self.generator.output_dir,
            module_name: self.generator.module_name,
            enable_sse_client: self.features.enable_sse_client,
            enable_async_client: self.features.enable_async_client,
            enable_specta: self.features.enable_specta,
            type_mappings: if self.type_mappings.is_empty() {
                super::generator::default_type_mappings()
            } else {
                self.type_mappings
            },
            streaming_config,
            nullable_field_overrides: self.nullable_overrides,
            schema_extensions: self.generator.schema_extensions,
            http_client_config,
            retry_config,
            tracing_enabled,
            auth_config,
        }
    }
}

/// Format validator errors into a readable message
fn format_validation_errors(errors: &validator::ValidationErrors) -> String {
    let mut messages = Vec::new();

    // Handle direct field errors
    for (field, field_errors) in errors.field_errors() {
        for error in field_errors {
            let msg = if let Some(message) = &error.message {
                format!("  - {}: {}", field, message)
            } else {
                format!("  - {}: validation failed (code: {})", field, error.code)
            };
            messages.push(msg);
        }
    }

    // Handle nested errors
    for (field, nested_errors) in errors.errors() {
        if let validator::ValidationErrorsKind::Struct(struct_errors) = nested_errors {
            let nested_msgs = format_validation_errors(struct_errors);
            if !nested_msgs.is_empty() {
                messages.push(format!("  - {} (nested):\n{}", field, nested_msgs));
            }
        }
    }

    messages.join("\n")
}

const EXAMPLE_CONFIG: &str = r#"[generator]
spec_path = "openapi.json"
output_dir = "src/generated"
module_name = "types"

[features]
enable_async_client = true

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 30

[http_client.retry]
max_retries = 3

[http_client.auth]
type = "Bearer"
header_name = "Authorization""#;
