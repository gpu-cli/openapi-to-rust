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
//! ## Client Selection Section (Optional)
//!
//! ```toml
//! [client]
//! # Shared selector grammar: operationId | "METHOD /path" | "tag:<name>"
//! operations = ["createResponse", "GET /models", "tag:Files"]
//! prune_models = true
//! ```
//!
//! Omitting `[client]`, or leaving `operations` empty, generates every HTTP
//! client operation. When pruning is enabled, model reachability is the union
//! of selected client and server operations.
//!
//! # Validation
//!
//! The configuration is validated on load:
//! - The input specification path is checked for existence
//! - Numeric ranges are enforced (timeout, retry counts, delays)
//! - Enum values are validated (auth types, event flow types)
//! - Required fields are checked
//! - Relative generator paths are resolved from the configuration file's directory
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

/// Root configuration loaded from TOML file
#[derive(Debug, Clone)]
pub struct ConfigFile {
    pub generator: GeneratorSection,
    pub features: FeaturesSection,
    pub http_client: Option<HttpClientSection>,
    pub streaming: Option<StreamingSection>,
    /// Server codegen opt-in. Absent or empty operations list ⇒ no
    /// server code emitted. See `docs/planning/server-codegen.md`.
    pub server: Option<ServerSection>,
    /// Optional HTTP-client operation scope. Absent means all operations.
    pub client: Option<ClientSection>,
    pub nullable_overrides: BTreeMap<String, bool>,
    /// Force a closed string-enum schema to be rendered as an extensible enum
    /// (with a `Custom(String)` fallback variant). Use when the spec under-
    /// declares the enum and the API returns values outside the declared set.
    /// Format: `"SchemaName" = true`. Mirror of `nullable_overrides`.
    pub extensible_enums: BTreeMap<String, bool>,
    pub type_mappings: BTreeMap<String, String>,
    /// Normalized type-mapping configuration.
    ///
    /// TOML deserialization accepts canonical `[generator.types]` and the
    /// temporary top-level `[types]` compatibility alias. Serialization always
    /// writes this value back in the canonical nested location.
    pub types: crate::type_mapping::TypeMappingConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorSection {
    /// OpenAPI input path or HTTPS URL. Relative filesystem paths are resolved
    /// from the directory containing the configuration file.
    pub spec_path: PathBuf,
    /// Generated-code destination. Relative paths are resolved from the
    /// directory containing the configuration file; it need not exist yet.
    pub output_dir: PathBuf,
    /// Informational label, not a directory or module path. The
    /// generator writes the same files (mod.rs, types.rs, server/*)
    /// regardless of this value. It shows up only in the generated
    /// mod.rs header doc comment as a hint and is used by the
    /// streaming codegen for naming the SSE client module. You
    /// mount the tree at whatever Rust module path you prefer.
    pub module_name: String,
    /// Schema extension files to merge into the main spec before codegen.
    /// Relative paths are resolved from the configuration file's directory.
    #[serde(default)]
    pub schema_extensions: Vec<PathBuf>,
    /// Additive operation-builder generation policy.
    #[serde(default)]
    pub builders: BuildersSection,
}

/// Configuration for additive `*_builder()` operation entry points.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct BuildersSection {
    /// Generate builders for operations above [`Self::threshold`].
    pub enabled: bool,
    /// Minimum optional-value count. Builders are emitted only when an
    /// operation has more optional values than this threshold.
    pub threshold: usize,
}

impl Default for BuildersSection {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 3,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeaturesSection {
    #[serde(default)]
    pub enable_sse_client: bool,
    #[serde(default)]
    pub enable_async_client: bool,
    #[serde(default)]
    pub enable_specta: bool,
    /// Generate a static operation registry with metadata for CLI/proxy routing
    #[serde(default)]
    pub enable_registry: bool,
    /// Generate only the operation registry (skip types, client, streaming)
    #[serde(default)]
    pub registry_only: bool,
}

/// Opt-in server codegen scope.
///
/// `operations` accepts three selector forms, parsed by
/// [`crate::server::Selector::parse`]:
///   - `operationId` (recommended)
///   - `METHOD /path`
///   - `tag:<name>`
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSection {
    /// Target framework. Only `"axum"` is currently supported.
    pub framework: String,
    /// Selectors picking which operations get server scaffolding.
    /// Empty ⇒ section is a no-op.
    #[serde(default)]
    pub operations: Vec<String>,
    /// Emit only the model types reachable (transitively) from all selected
    /// output scopes. When client and server generation coexist, pruning keeps
    /// the union of both operation sets plus configured streaming event roots.
    #[serde(default)]
    pub prune_models: bool,
}

/// Optional scope for generated HTTP-client operations.
///
/// Selectors use the same grammar as [`ServerSection::operations`]. An absent
/// section, or an empty `operations` list, preserves the historical behavior
/// of generating every operation. The section is ignored when the async HTTP
/// client is disabled and never filters the operation registry. Set
/// `prune_models = true` to also remove models outside the combined selected
/// client/server operation closure.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientSection {
    /// Selectors picking client methods: `operationId`, `METHOD /path`, or
    /// `tag:<name>`. Empty means all operations.
    #[serde(default)]
    pub operations: Vec<String>,
    /// Restrict `types.rs` to models reachable from the selected client
    /// operations plus any selected server operations.
    #[serde(default)]
    pub prune_models: bool,
}

impl ClientSection {
    /// Parse configured selectors using the shared client/server grammar.
    pub fn parsed_selectors(
        &self,
    ) -> Result<Vec<crate::server::Selector>, crate::server::SelectorParseError> {
        self.operations
            .iter()
            .map(|s| crate::server::Selector::parse(s))
            .collect()
    }
}

impl ServerSection {
    /// Parse each `operations` entry into a [`crate::server::Selector`].
    /// Returns the first parse error encountered.
    pub fn parsed_selectors(
        &self,
    ) -> Result<Vec<crate::server::Selector>, crate::server::SelectorParseError> {
        self.operations
            .iter()
            .map(|s| crate::server::Selector::parse(s))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HttpClientSection {
    pub base_url: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub auth: Option<AuthConfigSection>,
    #[serde(default)]
    pub headers: Vec<HeaderEntry>,
    pub retry: Option<RetryConfigSection>,
    pub tracing: Option<TracingConfigSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TracingConfigSection {
    #[serde(default = "default_tracing_enabled")]
    pub enabled: bool,
}

fn default_tracing_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetryConfigSection {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,
    #[serde(default = "default_max_delay_ms")]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthConfigSection {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub header_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderEntry {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StreamingSection {
    pub endpoints: Vec<StreamingEndpointSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StreamingEndpointSection {
    pub operation_id: String,
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
    pub event_union_type: String,
    pub content_type: Option<String>,
    pub event_flow: Option<EventFlowSection>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryParameterSection {
    pub name: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventFlowSection {
    #[serde(rename = "type")]
    pub flow_type: String,
    pub start_events: Option<Vec<String>>,
    pub delta_events: Option<Vec<String>>,
    pub stop_events: Option<Vec<String>>,
}

/// Serde-only representation of the TOML contract. Keeping this separate from
/// the public Rust API lets TOML use canonical `[generator.types]` while
/// preserving the long-standing normalized [`ConfigFile::types`] field.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFileWire {
    generator: GeneratorSectionWire,
    features: FeaturesSection,
    #[serde(default)]
    http_client: Option<HttpClientSection>,
    #[serde(default)]
    streaming: Option<StreamingSection>,
    #[serde(default)]
    server: Option<ServerSection>,
    #[serde(default)]
    client: Option<ClientSection>,
    #[serde(default)]
    nullable_overrides: BTreeMap<String, bool>,
    #[serde(default)]
    extensible_enums: BTreeMap<String, bool>,
    #[serde(default)]
    type_mappings: BTreeMap<String, String>,
    #[serde(default)]
    types: Option<crate::type_mapping::TypeMappingConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratorSectionWire {
    spec_path: PathBuf,
    output_dir: PathBuf,
    module_name: String,
    #[serde(default)]
    schema_extensions: Vec<PathBuf>,
    #[serde(default)]
    builders: BuildersSection,
    #[serde(default)]
    types: Option<crate::type_mapping::TypeMappingConfig>,
}

impl TryFrom<ConfigFileWire> for ConfigFile {
    type Error = String;

    fn try_from(wire: ConfigFileWire) -> Result<Self, Self::Error> {
        let types = match (wire.generator.types, wire.types) {
            (Some(_), Some(_)) => {
                return Err(
                    "Configuration contains both legacy [types] and canonical [generator.types]. Remove [types] and keep [generator.types]."
                        .to_string(),
                );
            }
            (Some(types), None) | (None, Some(types)) => types,
            (None, None) => crate::type_mapping::TypeMappingConfig::default(),
        };

        Ok(Self {
            generator: GeneratorSection {
                spec_path: wire.generator.spec_path,
                output_dir: wire.generator.output_dir,
                module_name: wire.generator.module_name,
                schema_extensions: wire.generator.schema_extensions,
                builders: wire.generator.builders,
            },
            features: wire.features,
            http_client: wire.http_client,
            streaming: wire.streaming,
            server: wire.server,
            client: wire.client,
            nullable_overrides: wire.nullable_overrides,
            extensible_enums: wire.extensible_enums,
            type_mappings: wire.type_mappings,
            types,
        })
    }
}

impl<'de> Deserialize<'de> for ConfigFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ConfigFileWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize)]
struct ConfigFileRef<'a> {
    generator: GeneratorSectionRef<'a>,
    features: &'a FeaturesSection,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_client: Option<&'a HttpClientSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    streaming: Option<&'a StreamingSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    server: Option<&'a ServerSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client: Option<&'a ClientSection>,
    nullable_overrides: &'a BTreeMap<String, bool>,
    extensible_enums: &'a BTreeMap<String, bool>,
    type_mappings: &'a BTreeMap<String, String>,
}

#[derive(Serialize)]
struct GeneratorSectionRef<'a> {
    spec_path: &'a Path,
    output_dir: &'a Path,
    module_name: &'a str,
    schema_extensions: &'a [PathBuf],
    builders: &'a BuildersSection,
    types: &'a crate::type_mapping::TypeMappingConfig,
}

impl Serialize for ConfigFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ConfigFileRef {
            generator: GeneratorSectionRef {
                spec_path: &self.generator.spec_path,
                output_dir: &self.generator.output_dir,
                module_name: &self.generator.module_name,
                schema_extensions: &self.generator.schema_extensions,
                builders: &self.generator.builders,
                types: &self.types,
            },
            features: &self.features,
            http_client: self.http_client.as_ref(),
            streaming: self.streaming.as_ref(),
            server: self.server.as_ref(),
            client: self.client.as_ref(),
            nullable_overrides: &self.nullable_overrides,
            extensible_enums: &self.extensible_enums,
            type_mappings: &self.type_mappings,
        }
        .serialize(serializer)
    }
}

fn resolve_relative_path(config_dir: &Path, path: &mut PathBuf) {
    if path.is_relative() && !path.to_string_lossy().contains("://") {
        *path = config_dir.join(&*path);
    }
}

fn inspect_type_config_layout(value: &toml::Value) -> Result<(), GeneratorError> {
    let legacy_types = value.get("types").is_some();
    let canonical_types = value
        .get("generator")
        .and_then(|generator| generator.get("types"));

    if legacy_types && canonical_types.is_some() {
        return Err(GeneratorError::ValidationError(
            "Configuration contains both legacy [types] and canonical [generator.types]. Remove [types] and keep [generator.types]."
                .to_string(),
        ));
    }

    if canonical_types
        .and_then(|types| types.get("strategies"))
        .is_some()
    {
        return Err(GeneratorError::ValidationError(
            "[generator.types.strategies] is obsolete. Move its fields directly under [generator.types]. Use snake_case keys such as date_time (not date-time); valid byte values are string, base64, and vec_u8 (for example: byte = \"base64\")."
                .to_string(),
        ));
    }

    Ok(())
}

impl ConfigFile {
    /// Load and validate configuration from a TOML file.
    ///
    /// Relative `spec_path`, `output_dir`, and `schema_extensions` values are
    /// resolved against the directory containing `path`, independent of the
    /// process's current working directory.
    pub fn load(path: &Path) -> Result<Self, GeneratorError> {
        let config_path = path.canonicalize().map_err(|e| GeneratorError::FileError {
            message: format!("Failed to resolve config file '{}': {}", path.display(), e),
        })?;
        let config_dir = config_path
            .parent()
            .ok_or_else(|| GeneratorError::FileError {
                message: format!(
                    "Config file '{}' has no parent directory",
                    config_path.display()
                ),
            })?;
        let content =
            std::fs::read_to_string(&config_path).map_err(|e| GeneratorError::FileError {
                message: format!(
                    "Failed to read config file '{}': {}",
                    config_path.display(),
                    e
                ),
            })?;

        let value: toml::Value =
            toml::from_str(&content).map_err(|e| GeneratorError::FileError {
                message: format!(
                    "Failed to parse TOML config: {}\n\nExample config:\n{}",
                    e, EXAMPLE_CONFIG
                ),
            })?;
        inspect_type_config_layout(&value)?;

        let mut config: ConfigFile =
            toml::from_str(&content).map_err(|e| GeneratorError::FileError {
                message: format!(
                    "Failed to parse TOML config: {}\n\nExample config:\n{}",
                    e, EXAMPLE_CONFIG
                ),
            })?;

        resolve_relative_path(config_dir, &mut config.generator.spec_path);
        resolve_relative_path(config_dir, &mut config.generator.output_dir);
        for extension in &mut config.generator.schema_extensions {
            resolve_relative_path(config_dir, extension);
        }

        config.validate()?;

        Ok(config)
    }

    fn validate(&self) -> Result<(), GeneratorError> {
        let mut errors = Vec::new();

        let spec_source = self.generator.spec_path.to_string_lossy();
        if crate::cli::is_remote_spec(&spec_source) {
            if let Err(error) = crate::cli::validate_remote_spec_url(&spec_source) {
                errors.push(format!("generator.spec_path: {error}"));
            }
        } else if spec_source.contains("://") {
            let error = crate::cli::validate_remote_spec_url(&spec_source)
                .err()
                .unwrap_or_else(|| "unsupported remote OpenAPI URL".to_string());
            errors.push(format!("generator.spec_path: {error}"));
        } else if !self.generator.spec_path.exists() {
            errors.push(format!(
                "generator.spec_path: OpenAPI spec file not found: {}. Ensure spec_path points to a valid OpenAPI JSON or YAML file.",
                self.generator.spec_path.display()
            ));
        }
        if self.generator.module_name.is_empty() {
            errors.push("generator.module_name: module_name cannot be empty".to_string());
        }

        if let Some(server) = &self.server
            && server.framework != "axum"
        {
            errors.push(format!(
                "server.framework: framework must be \"axum\" (got \"{}\"); other frameworks are not supported yet",
                server.framework
            ));
        }

        if let Some(client) = &self.client {
            for (index, selector) in client.operations.iter().enumerate() {
                if let Err(error) = crate::server::Selector::parse(selector) {
                    errors.push(format!("client.operations[{index}]: {error}"));
                }
            }
        }
        if let Some(server) = &self.server {
            for (index, selector) in server.operations.iter().enumerate() {
                if let Err(error) = crate::server::Selector::parse(selector) {
                    errors.push(format!("server.operations[{index}]: {error}"));
                }
            }
        }

        if let Some(http) = &self.http_client {
            if let Some(base_url) = &http.base_url
                && reqwest::Url::parse(base_url).is_err()
            {
                errors.push("http_client.base_url: base_url must be a valid URL".to_string());
            }
            if let Some(timeout) = http.timeout_seconds
                && !(1..=3600).contains(&timeout)
            {
                errors.push(
                    "http_client.timeout_seconds: timeout_seconds must be between 1 and 3600"
                        .to_string(),
                );
            }
            if let Some(auth) = &http.auth {
                if !matches!(auth.auth_type.as_str(), "Bearer" | "ApiKey" | "Custom") {
                    errors.push(format!(
                        "http_client.auth.type: Invalid auth type '{}'. Must be one of: Bearer, ApiKey, Custom",
                        auth.auth_type
                    ));
                }
                if auth.header_name.is_empty() {
                    errors.push(
                        "http_client.auth.header_name: header_name cannot be empty".to_string(),
                    );
                }
            }
            for (index, header) in http.headers.iter().enumerate() {
                if header.name.is_empty() {
                    errors.push(format!(
                        "http_client.headers[{index}].name: header name cannot be empty"
                    ));
                }
            }
            if let Some(retry) = &http.retry {
                if retry.max_retries > 10 {
                    errors.push(
                        "http_client.retry.max_retries: max_retries must be between 0 and 10"
                            .to_string(),
                    );
                }
                if !(100..=10000).contains(&retry.initial_delay_ms) {
                    errors.push(
                        "http_client.retry.initial_delay_ms: initial_delay_ms must be between 100 and 10000"
                            .to_string(),
                    );
                }
                if !(1000..=300000).contains(&retry.max_delay_ms) {
                    errors.push(
                        "http_client.retry.max_delay_ms: max_delay_ms must be between 1000 and 300000"
                            .to_string(),
                    );
                }
            }
        }

        if let Some(streaming) = &self.streaming {
            for (index, endpoint) in streaming.endpoints.iter().enumerate() {
                let prefix = format!("streaming.endpoints[{index}]");
                if endpoint.operation_id.is_empty() {
                    errors.push(format!("{prefix}.operation_id: must not be empty"));
                }
                if endpoint.path.is_empty() {
                    errors.push(format!("{prefix}.path: must not be empty"));
                }
                if endpoint.event_union_type.is_empty() {
                    errors.push(format!("{prefix}.event_union_type: must not be empty"));
                }
                for (query_index, query) in endpoint.query_parameters.iter().enumerate() {
                    if query.name.is_empty() {
                        errors.push(format!(
                            "{prefix}.query_parameters[{query_index}].name: must not be empty"
                        ));
                    }
                }
                if let Some(flow) = &endpoint.event_flow
                    && !matches!(
                        flow.flow_type.as_str(),
                        "StartDeltaStop" | "start_delta_stop" | "Continuous"
                    )
                {
                    errors.push(format!(
                        "{prefix}.event_flow.type: Invalid event flow type '{}'. Must be one of: StartDeltaStop, Continuous",
                        flow.flow_type
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(GeneratorError::ValidationError(format!(
                "Configuration validation failed:\n  - {}",
                errors.join("\n  - ")
            )))
        }
    }

    /// Convert to internal GeneratorConfig
    pub fn into_generator_config(self) -> GeneratorConfig {
        use crate::http_config::{AuthConfig, HttpClientConfig, RetryConfig};

        let types = self.types;

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
                            "StartDeltaStop" | "start_delta_stop" => EventFlow::StartDeltaStop {
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
            extensible_enum_overrides: self.extensible_enums,
            schema_extensions: self.generator.schema_extensions,
            http_client_config,
            retry_config,
            tracing_enabled,
            auth_config,
            enable_registry: self.features.enable_registry,
            registry_only: self.features.registry_only,
            types,
            builders: self.generator.builders,
            server: self.server,
            client: self.client,
        }
    }
}

const EXAMPLE_CONFIG: &str = r#"[generator]
spec_path = "openapi.json"
output_dir = "src/generated"
module_name = "types"

[generator.builders]
enabled = true
threshold = 3

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
