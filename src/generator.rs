use crate::{GeneratorError, Result, analysis::SchemaAnalysis, streaming::StreamingConfig};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Parse a Rust type string (possibly with generics, e.g.
/// `chrono::DateTime<chrono::Utc>`) into a `TokenStream`. The pre-Q2
/// ad-hoc `::`-splitter choked on `<` and `>`; `syn::parse_str` handles
/// every valid type expression. Errors here mean the [`TypeMapper`]
/// produced a string that doesn't parse as a Rust type — a generator
/// bug, surfaced as a `GeneratorError::CodeGenError`.
///
/// [`TypeMapper`]: crate::type_mapping::TypeMapper
fn parse_rust_type(rust_type: &str) -> Result<TokenStream> {
    let parsed: syn::Type = syn::parse_str(rust_type).map_err(|e| {
        GeneratorError::CodeGenError(format!(
            "TypeMapper produced un-parseable type `{rust_type}`: {e}"
        ))
    })?;
    Ok(quote! { #parsed })
}

/// Q2.4 — render OpenAPI constraint annotations as a single-line
/// human-readable doc comment, e.g.
///   "Constraint: minimum=0, maximum=100, pattern=`^foo$`"
///
/// The pattern is wrapped in backticks so backticks/braces inside
/// it don't trip prettyplease/rustdoc parsing. Triple-slash and
/// `*/` sequences are escaped so embedded patterns can't terminate
/// the surrounding doc comment / block comment.
fn format_constraints_doc(c: &crate::analysis::PropertyConstraints) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(v) = c.minimum {
        parts.push(format!("minimum={}", strip_trailing_zero(v)));
    }
    if let Some(v) = c.maximum {
        parts.push(format!("maximum={}", strip_trailing_zero(v)));
    }
    if let Some(v) = c.exclusive_minimum {
        parts.push(format!("exclusiveMinimum={}", strip_trailing_zero(v)));
    }
    if let Some(v) = c.exclusive_maximum {
        parts.push(format!("exclusiveMaximum={}", strip_trailing_zero(v)));
    }
    if let Some(v) = c.multiple_of {
        parts.push(format!("multipleOf={}", strip_trailing_zero(v)));
    }
    if let Some(v) = c.min_length {
        parts.push(format!("minLength={v}"));
    }
    if let Some(v) = c.max_length {
        parts.push(format!("maxLength={v}"));
    }
    if let Some(v) = c.min_items {
        parts.push(format!("minItems={v}"));
    }
    if let Some(v) = c.max_items {
        parts.push(format!("maxItems={v}"));
    }
    if c.unique_items == Some(true) {
        parts.push("uniqueItems=true".to_string());
    }
    if let Some(p) = &c.pattern {
        // Insert a zero-width-space inside `///` and `*/` so they
        // can't terminate the surrounding doc/block comment. Using
        // the `\u{200B}` escape (vs. a literal U+200B) keeps clippy's
        // `invisible_characters` lint happy.
        let safe = p.replace("///", "/\u{200B}//").replace("*/", "*\u{200B}/");
        parts.push(format!("pattern=`{safe}`"));
    }

    format!("Constraint: {}", parts.join(", "))
}

/// `1.0` and `1` should both render as `1` in doc comments.
/// `1.5` stays `1.5`.
fn strip_trailing_zero(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Info about schemas that are variants in discriminated unions
#[derive(Clone)]
pub(crate) struct DiscriminatedVariantInfo {
    /// The discriminator field name (e.g., "type")
    pub(crate) discriminator_field: String,
    /// The const value of the discriminator (e.g., "text")
    pub(crate) discriminator_value: String,
    /// Whether the parent union is untagged
    pub(crate) is_parent_untagged: bool,
}

/// One object property after discriminator filtering and Rust-identifier
/// disambiguation. Struct fields, request-model constructors, and builders
/// all consume this shared projection so their names and types cannot drift.
pub(crate) struct EmittedObjectProperty<'a> {
    pub(crate) wire_name: &'a str,
    pub(crate) property: &'a crate::analysis::PropertyInfo,
    pub(crate) ident: syn::Ident,
    pub(crate) is_required: bool,
    pub(crate) field_type: TokenStream,
}

/// Shared lookups for one types.rs generation pass. Large schemas can contain
/// thousands of operations and types, so request-root and type-name queries
/// must not rescan the full analysis for each emitted object.
struct TypeGenerationIndex {
    request_body_roots: std::collections::HashSet<String>,
    reserved_type_names: std::collections::HashSet<String>,
}

struct TypeGenerationContext<'a> {
    discriminated_variants: &'a BTreeMap<String, DiscriminatedVariantInfo>,
    index: &'a TypeGenerationIndex,
}

#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Path to OpenAPI specification file
    pub spec_path: PathBuf,
    /// Output directory for generated code (e.g., "src/gen")
    pub output_dir: PathBuf,
    /// Informational label for the generated module. Does NOT pick
    /// the on-disk directory (that's `output_dir`) or the Rust module
    /// path the user mounts the tree at — both of those are the
    /// user's choice. The label is surfaced in the generated mod.rs
    /// header as a hint and is otherwise used only by the streaming
    /// codegen for naming the SSE client module.
    pub module_name: String,
    /// Enable SSE streaming client generation
    pub enable_sse_client: bool,
    /// Enable async HTTP client generation
    pub enable_async_client: bool,
    /// Enable Specta type derives for frontend integration
    pub enable_specta: bool,
    /// Custom type mappings
    pub type_mappings: BTreeMap<String, String>,
    /// Optional streaming configuration for SSE client generation
    pub streaming_config: Option<StreamingConfig>,
    /// Fields that should be treated as nullable even if not marked in the spec
    /// Format: "SchemaName.fieldName" -> true
    pub nullable_field_overrides: BTreeMap<String, bool>,
    /// String-enum schemas that should be rendered as extensible (with a
    /// `Custom(String)` fallback variant) instead of closed enums. Useful when
    /// the spec declares a fixed set of values but the API actually returns
    /// values outside that set (real-world drift: Cloudflare's r2_bucket_location
    /// declares lowercase but returns uppercase).
    /// Format: "SchemaName" -> true
    pub extensible_enum_overrides: BTreeMap<String, bool>,
    /// Additional schema extension files to merge into the main spec
    /// These files will be merged additively using simple JSON object merging
    pub schema_extensions: Vec<PathBuf>,
    /// HTTP client configuration
    pub http_client_config: Option<crate::http_config::HttpClientConfig>,
    /// Retry configuration for HTTP requests
    pub retry_config: Option<crate::http_config::RetryConfig>,
    /// Enable request/response tracing
    pub tracing_enabled: bool,
    /// Authentication configuration
    pub auth_config: Option<crate::http_config::AuthConfig>,
    /// Enable operation registry generation (static metadata for CLI/proxy routing)
    pub enable_registry: bool,
    /// Generate only the operation registry (skip types, client, streaming)
    pub registry_only: bool,
    /// Per-format type-mapping strategies driven by the `[generator.types]`
    /// TOML section. Q2.0 introduces this field; with the default value
    /// every mapping preserves pre-refactor behavior.
    pub types: crate::type_mapping::TypeMappingConfig,
    /// Additive operation-builder generation policy.
    pub builders: crate::config::BuildersSection,
    /// Opt-in server codegen scope. `None` ⇒ emit no server code.
    /// Set by the `[server]` section in the TOML config.
    pub server: Option<crate::config::ServerSection>,
    /// Optional HTTP-client operation scope. `None` or an empty selector list
    /// preserves generation of every operation.
    pub client: Option<crate::config::ClientSection>,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            spec_path: "openapi.json".into(),
            output_dir: "src/gen".into(),
            module_name: "api_types".to_string(),
            enable_sse_client: true,
            enable_async_client: true,
            enable_specta: false,
            type_mappings: default_type_mappings(),
            streaming_config: None,
            nullable_field_overrides: BTreeMap::new(),
            extensible_enum_overrides: BTreeMap::new(),
            schema_extensions: Vec::new(),
            http_client_config: None,
            retry_config: None,
            tracing_enabled: true,
            auth_config: None,
            enable_registry: false,
            registry_only: false,
            types: crate::type_mapping::TypeMappingConfig::default(),
            builders: crate::config::BuildersSection::default(),
            server: None,
            client: None,
        }
    }
}

impl GeneratorConfig {
    /// Adopt the document's `servers[0].url` as the client's default base URL
    /// when configuration didn't supply one.
    ///
    /// The spec already states where the API lives; making every user restate
    /// it in TOML (or discover at runtime that requests go nowhere) is friction
    /// with no upside. Explicit configuration always wins.
    ///
    /// Two server URLs are deliberately ignored: relative ones (`/v1`), which
    /// are meaningless without an origin, and templated ones containing `{}`
    /// server variables, which are not usable until substituted.
    pub fn apply_spec_server_default(&mut self, spec: &serde_json::Value) {
        let already_configured = self
            .http_client_config
            .as_ref()
            .and_then(|http| http.base_url.as_deref())
            .is_some_and(|url| !url.is_empty());
        if already_configured {
            return;
        }

        let Some(url) = spec
            .pointer("/servers/0/url")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .filter(|url| !url.starts_with('/'))
            .filter(|url| !url.contains('{'))
        else {
            return;
        };

        match self.http_client_config.as_mut() {
            Some(http) => http.base_url = Some(url.to_string()),
            None => {
                self.http_client_config = Some(crate::http_config::HttpClientConfig {
                    base_url: Some(url.to_string()),
                    timeout_seconds: None,
                    default_headers: Default::default(),
                })
            }
        }
    }
}

pub fn default_type_mappings() -> BTreeMap<String, String> {
    let mut mappings = BTreeMap::new();
    mappings.insert("integer".to_string(), "i64".to_string());
    mappings.insert("number".to_string(), "f64".to_string());
    mappings.insert("string".to_string(), "String".to_string());
    mappings.insert("boolean".to_string(), "bool".to_string());
    mappings
}

/// Represents a generated file
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    /// Relative path from output directory (e.g., "types.rs", "streaming.rs")
    pub path: PathBuf,
    /// Generated Rust code content
    pub content: String,
}

/// Result of code generation containing multiple files
#[derive(Debug, Clone)]
pub struct GenerationResult {
    /// All generated files
    pub files: Vec<GeneratedFile>,
    /// Generated mod.rs content that exports all modules
    pub mod_file: GeneratedFile,
    /// Complete direct dependencies for the exact files in this result,
    /// including required crate features and compatible versions. The CLI
    /// writes these as `REQUIRED_DEPS.toml` next to the generated module.
    pub required_deps: Vec<crate::type_mapping::DepRequirement>,
    /// Number of schemas removed by opt-in client/server model pruning.
    pub pruned_schemas: usize,
}

#[derive(Debug)]
struct OperationScopes {
    /// `None` means the enabled HTTP client keeps every operation.
    client_ids: Option<std::collections::BTreeSet<String>>,
    server_ids: std::collections::BTreeSet<String>,
    streaming_ids: std::collections::BTreeSet<String>,
    prune_models: bool,
    extra_schema_roots: Vec<String>,
}

pub struct CodeGenerator {
    config: GeneratorConfig,
    source_provenance: Option<String>,
}

impl CodeGenerator {
    pub fn new(config: GeneratorConfig) -> Self {
        Self {
            config,
            source_provenance: None,
        }
    }

    /// Attach a sanitized source label to generated module headers.
    pub fn with_source_provenance(mut self, source: impl Into<String>) -> Self {
        self.source_provenance = Some(source.into());
        self
    }

    /// Get reference to the generator configuration
    pub fn config(&self) -> &GeneratorConfig {
        &self.config
    }

    pub(crate) fn provenance_attribute(&self) -> TokenStream {
        self.source_provenance
            .as_ref()
            .map(|source| {
                let provenance = format!(
                    " Generated by openapi-to-rust v{}. Source OpenAPI document: {source}",
                    env!("CARGO_PKG_VERSION")
                );
                quote! { #![doc = #provenance] }
            })
            .unwrap_or_default()
    }

    /// Generate all files for the API
    pub fn generate_all(&self, analysis: &mut SchemaAnalysis) -> Result<GenerationResult> {
        // Resolve client/server selectors exactly once for this generation.
        // The same scopes drive client artifacts and the union model closure.
        let scopes = self.resolve_operation_scopes(analysis)?;
        let pruned_schemas = self.prune_models_to_scopes(analysis, &scopes);
        let mut files = Vec::new();

        if !self.config.registry_only {
            // Generate types file
            let types_content = self.generate_types(analysis)?;
            files.push(GeneratedFile {
                path: "types.rs".into(),
                content: types_content,
            });

            // Generate streaming client if configured
            if self.config.enable_sse_client
                && let Some(ref streaming_config) = self.config.streaming_config
            {
                if streaming_config.generate_client && !streaming_config.event_parser_helpers {
                    return Err(GeneratorError::ValidationError(
                        "streaming generate_client=true requires event_parser_helpers=true"
                            .to_string(),
                    ));
                }
                let streaming_content =
                    self.generate_streaming_client(streaming_config, analysis)?;
                files.push(GeneratedFile {
                    path: "streaming.rs".into(),
                    content: streaming_content,
                });
            }

            // Generate HTTP client if enabled
            if self.config.enable_async_client {
                let operations = self.client_operations(analysis, scopes.client_ids.as_ref());
                let http_content =
                    self.generate_http_client_for_operations(analysis, &operations)?;
                files.push(GeneratedFile {
                    path: "client.rs".into(),
                    content: http_content,
                });
            }
        }

        // Generate operation registry if enabled
        if self.config.enable_registry || self.config.registry_only {
            let registry_content = self.generate_registry(analysis)?;
            files.push(GeneratedFile {
                path: "registry.rs".into(),
                content: registry_content,
            });
        }

        // Server files are part of the same generation result so module wiring,
        // dependency collection, and disk writes cannot drift from the CLI's
        // post-processing path.
        if !self.config.registry_only
            && let Some(server) = self
                .config
                .server
                .as_ref()
                .filter(|server| !server.operations.is_empty())
        {
            let server_files =
                crate::server::codegen::ServerCodegen::new(&self.config, analysis, server)
                    .with_source_provenance(self.source_provenance.as_deref())
                    .generate()
                    .map_err(|error| {
                        GeneratorError::CodeGenError(format!(
                            "server code generation failed: {error}"
                        ))
                    })?;
            files.extend(server_files);
        }

        // Generate mod.rs file
        let mod_content = self.generate_mod_file(&files)?;
        let mod_file = GeneratedFile {
            path: "mod.rs".into(),
            content: mod_content,
        };

        let required_deps = crate::type_mapping::collect_generated_dep_requirements(
            files.iter().map(|file| file.content.as_str()),
            self.config.enable_specta,
        );

        Ok(GenerationResult {
            files,
            mod_file,
            required_deps,
            pruned_schemas,
        })
    }

    /// Generate just the types (legacy single-file interface)
    pub fn generate(&self, analysis: &mut SchemaAnalysis) -> Result<String> {
        self.generate_types(analysis)
    }

    /// Generate the types.rs file content
    fn generate_types(&self, analysis: &mut SchemaAnalysis) -> Result<String> {
        let provenance_attribute = self.provenance_attribute();
        let mut type_definitions = TokenStream::new();

        // Collect all schemas that are used as variants in discriminated unions
        // Only include direct references, not schemas wrapped in allOf
        let mut discriminated_variant_info: BTreeMap<String, DiscriminatedVariantInfo> =
            BTreeMap::new();

        // Sort schemas for deterministic processing
        let mut sorted_schemas: Vec<_> = analysis.schemas.iter().collect();
        sorted_schemas.sort_by_key(|(name, _)| name.as_str());

        for (_parent_name, schema) in sorted_schemas {
            if let crate::analysis::SchemaType::DiscriminatedUnion {
                variants,
                discriminator_field,
            } = &schema.schema_type
            {
                // Check if this discriminated union will be generated as untagged
                let is_parent_untagged =
                    self.should_use_untagged_discriminated_union(schema, analysis);

                for variant in variants {
                    // Only add if it's a direct reference to a schema that will have the discriminator field
                    // Check if the schema exists and has the discriminator field as a property
                    if let Some(variant_schema) = analysis.schemas.get(&variant.type_name) {
                        if let crate::analysis::SchemaType::Object { properties, .. } =
                            &variant_schema.schema_type
                        {
                            if properties.contains_key(discriminator_field) {
                                discriminated_variant_info.insert(
                                    variant.type_name.clone(),
                                    DiscriminatedVariantInfo {
                                        discriminator_field: discriminator_field.clone(),
                                        discriminator_value: variant.discriminator_value.clone(),
                                        is_parent_untagged,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        let type_index = self.type_generation_index(analysis);
        let type_context = TypeGenerationContext {
            discriminated_variants: &discriminated_variant_info,
            index: &type_index,
        };

        // Generate types based on dependency order
        let generation_order = analysis.dependencies.topological_sort()?;

        // Defensive layer: track emitted Rust type names so that two
        // analyzed schemas which sanitize to the same Rust ident don't
        // produce two definitions (E0119 conflicting impls / E0428 name
        // defined multiple times). The first occurrence wins; later
        // occurrences are silently dropped. Schema-name uniqueness at the
        // analysis layer is a follow-up; this stops the generated file from
        // failing to compile.
        let mut emitted_rust_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let mut processed = std::collections::HashSet::new();

        // First, generate schemas in dependency order
        for schema_name in generation_order {
            if let Some(schema) = analysis.schemas.get(&schema_name) {
                let rust_name = self.to_rust_type_name(&schema.name);
                if !emitted_rust_names.insert(rust_name) {
                    processed.insert(schema_name);
                    continue;
                }
                let type_def = self.generate_type_definition(schema, analysis, &type_context)?;
                if !type_def.is_empty() {
                    type_definitions.extend(type_def);
                }
                processed.insert(schema_name);
            }
        }

        // Then generate any remaining schemas not in dependency graph
        let mut remaining_schemas: Vec<_> = analysis
            .schemas
            .iter()
            .filter(|(name, _)| !processed.contains(*name))
            .collect();
        remaining_schemas.sort_by_key(|(name, _)| name.as_str());

        for (_schema_name, schema) in remaining_schemas {
            let rust_name = self.to_rust_type_name(&schema.name);
            if !emitted_rust_names.insert(rust_name) {
                continue;
            }
            let type_def = self.generate_type_definition(schema, analysis, &type_context)?;
            if !type_def.is_empty() {
                type_definitions.extend(type_def);
            }
        }

        // Helper modules emitted only when the analyzer actually
        // referenced their codecs. Avoids polluting every generated
        // file (and every snapshot) with dead code for specs that
        // don't use `format: byte`.
        let base64_helper = if analysis
            .used_type_features
            .contains(crate::type_mapping::TypeFeature::Base64)
        {
            let engine = match self.config.types.byte {
                crate::type_mapping::ByteStrategy::Base64UrlUnpadded => {
                    quote::format_ident!("URL_SAFE_NO_PAD")
                }
                _ => quote::format_ident!("STANDARD"),
            };
            quote! {
                /// base64 codec for `Vec<u8>` fields produced from
                /// `format: byte`. Used via `#[serde(with = "base64_serde")]`
                /// for required/non-null fields; `with = "base64_serde::option"`
                /// for the Option<Vec<u8>> case.
                mod base64_serde {
                    use base64::{Engine as _, engine::general_purpose::#engine as ENGINE};
                    use serde::{Deserialize, Deserializer, Serializer};

                    pub fn serialize<S: Serializer>(
                        bytes: &Vec<u8>,
                        ser: S,
                    ) -> Result<S::Ok, S::Error> {
                        ser.serialize_str(&ENGINE.encode(bytes))
                    }

                    pub fn deserialize<'de, D: Deserializer<'de>>(
                        de: D,
                    ) -> Result<Vec<u8>, D::Error> {
                        let s = String::deserialize(de)?;
                        ENGINE
                            .decode(s.as_bytes())
                            .map_err(serde::de::Error::custom)
                    }

                    /// Codec for Option<Vec<u8>> fields (optional /
                    /// nullable `format: byte`). serde dispatches on
                    /// the field type; without this submodule the
                    /// `?` operator in the generated code would fail
                    /// to convert Vec<u8> to Option<Vec<u8>>.
                    pub mod option {
                        use super::*;
                        use serde::{Deserialize, Deserializer, Serializer};

                        pub fn serialize<S: Serializer>(
                            opt: &Option<Vec<u8>>,
                            ser: S,
                        ) -> Result<S::Ok, S::Error> {
                            match opt {
                                Some(bytes) => super::serialize(bytes, ser),
                                None => ser.serialize_none(),
                            }
                        }

                        pub fn deserialize<'de, D: Deserializer<'de>>(
                            de: D,
                        ) -> Result<Option<Vec<u8>>, D::Error> {
                            let opt = Option::<String>::deserialize(de)?;
                            opt.map(|s| {
                                ENGINE
                                    .decode(s.as_bytes())
                                    .map_err(serde::de::Error::custom)
                            })
                            .transpose()
                        }
                    }
                }
            }
        } else {
            TokenStream::new()
        };

        // `time::Date` / `time::Time` have no built-in serde codec
        // in the `time` crate (`time::serde::iso8601` is
        // OffsetDateTime-only — GH #25), so declare one per type via
        // the `format_description!` macro. It expands to a module
        // (with an `::option` submodule) referenced from fields as
        // `#[serde(with = "time_date_format")]` etc.
        let time_date_helper = if analysis
            .used_type_features
            .contains(crate::type_mapping::TypeFeature::TimeDate)
        {
            quote! {
                time::serde::format_description!(
                    time_date_format,
                    Date,
                    "[year]-[month]-[day]"
                );
            }
        } else {
            TokenStream::new()
        };

        // RFC 3339 partial-time. `[optional [...]]` groups always
        // format their contents, so whole seconds serialize with a
        // trailing ".0" — in exchange, parsing accepts inputs both
        // with and without fractional seconds.
        let time_time_helper = if analysis
            .used_type_features
            .contains(crate::type_mapping::TypeFeature::TimeTime)
        {
            quote! {
                time::serde::format_description!(
                    version = 2,
                    time_time_format,
                    Time,
                    "[hour]:[minute]:[second][optional [.[subsecond]]]"
                );
            }
        } else {
            TokenStream::new()
        };

        // Generate file with imports and types (no module wrapper).
        let generated = quote! {
            //! Generated types from OpenAPI specification
            //!
            //! This file contains all the generated types for the API.
            //! Do not edit manually - regenerate using the appropriate script.

            #provenance_attribute

            #![allow(clippy::large_enum_variant)]
            #![allow(clippy::format_in_format_args)]
            #![allow(clippy::let_unit_value)]
            #![allow(unreachable_patterns)]

            use serde::{Deserialize, Serialize};

            #base64_helper

            #time_date_helper

            #time_time_helper

            #type_definitions
        };

        // Format the generated code
        let syntax_tree = syn::parse2::<syn::File>(generated).map_err(|e| {
            GeneratorError::CodeGenError(format!("Failed to parse generated code: {e}"))
        })?;

        let formatted = prettyplease::unparse(&syntax_tree);

        Ok(formatted)
    }

    /// Generate streaming client code
    fn generate_streaming_client(
        &self,
        streaming_config: &StreamingConfig,
        analysis: &SchemaAnalysis,
    ) -> Result<String> {
        let mut client_code = TokenStream::new();
        let provenance_attribute = self.provenance_attribute();

        // Generate imports
        let imports = quote! {
            //! Generated streaming client for SSE (Server-Sent Events)
            //!
            //! This file contains the streaming client implementation.
            //! Do not edit manually - regenerate using the appropriate script.
            #provenance_attribute
            #![allow(clippy::format_in_format_args)]
            #![allow(clippy::let_unit_value)]
            #![allow(unused_mut)]

            use super::types::*;
            use async_trait::async_trait;
            use futures_util::{Stream, StreamExt};
            use std::pin::Pin;
            use std::time::Duration;
            use reqwest::header::{HeaderMap, HeaderValue};
            use tracing::{debug, error, info, warn, instrument};
        };
        client_code.extend(imports);

        // Generate error types
        if streaming_config.generate_client {
            let error_types = self.generate_streaming_error_types()?;
            client_code.extend(error_types);
        }

        // Generate client trait for each endpoint
        for endpoint in &streaming_config.endpoints {
            let trait_code = self.generate_endpoint_trait(endpoint, analysis)?;
            client_code.extend(trait_code);
        }

        // Generate client implementation
        if streaming_config.generate_client {
            let client_impl = self.generate_streaming_client_impl(streaming_config, analysis)?;
            client_code.extend(client_impl);
        }

        // Generate SSE parsing utilities
        if streaming_config.event_parser_helpers {
            let parser_code = self.generate_sse_parser_utilities(streaming_config)?;
            client_code.extend(parser_code);
        }

        // Generate reconnection utilities if configured
        if let Some(reconnect_config) = &streaming_config.reconnection_config {
            let reconnect_code = self.generate_reconnection_utilities(reconnect_config)?;
            client_code.extend(reconnect_code);
        }

        let syntax_tree = syn::parse2::<syn::File>(client_code).map_err(|e| {
            GeneratorError::CodeGenError(format!("Failed to parse streaming client code: {e}"))
        })?;

        Ok(prettyplease::unparse(&syntax_tree))
    }

    /// Generate HTTP client code for regular (non-streaming) requests.
    ///
    /// This standalone entry point honors `[client].operations` but does not
    /// validate unrelated server or streaming scopes. Use [`Self::generate_all`]
    /// when generating the complete configured output set.
    pub fn generate_http_client(&self, analysis: &SchemaAnalysis) -> Result<String> {
        let client_ids = self.resolve_client_operation_ids(analysis)?;
        let operations = self.client_operations(analysis, client_ids.as_ref());
        self.generate_http_client_for_operations(analysis, &operations)
    }

    fn generate_http_client_for_operations(
        &self,
        analysis: &SchemaAnalysis,
        operations: &[&crate::analysis::OperationInfo],
    ) -> Result<String> {
        let provenance_attribute = self.provenance_attribute();
        let error_types = self.generate_http_error_types();
        let client_struct = self.generate_http_client_struct();
        let operation_methods = self.generate_operation_methods_for(analysis, operations);

        let generated = quote! {
            //! Generated HTTP client for regular API requests
            //!
            //! This file contains the HTTP client implementation for GET, POST, etc.
            //! Do not edit manually - regenerate using the appropriate script.
            #provenance_attribute
            #![allow(clippy::format_in_format_args)]
            #![allow(clippy::let_unit_value)]

            use super::types::*;

            #error_types

            #client_struct

            #operation_methods
        };

        let syntax_tree = syn::parse2::<syn::File>(generated).map_err(|e| {
            GeneratorError::CodeGenError(format!("Failed to parse HTTP client code: {e}"))
        })?;

        Ok(prettyplease::unparse(&syntax_tree))
    }

    fn resolve_operation_scopes(&self, analysis: &SchemaAnalysis) -> Result<OperationScopes> {
        let client_ids = if self.config.enable_async_client && !self.config.registry_only {
            self.resolve_client_operation_ids(analysis)?
        } else {
            None
        };

        let server_ids = match &self.config.server {
            Some(server) if !server.operations.is_empty() => {
                crate::server::resolve_operation_selectors(&server.operations, analysis)
                    .map_err(|error| {
                        GeneratorError::ValidationError(format!(
                            "Invalid [server].operations: {error}"
                        ))
                    })?
                    .operations
                    .into_iter()
                    .map(|operation| operation.operation_id)
                    .collect()
            }
            _ => Default::default(),
        };

        let streaming_ids = if self.config.registry_only || !self.config.enable_sse_client {
            Default::default()
        } else if let Some(streaming) = &self.config.streaming_config {
            let mut ids = std::collections::BTreeSet::new();
            for (index, endpoint) in streaming.endpoints.iter().enumerate() {
                let resolution =
                    crate::server::resolve_operation_id(&endpoint.operation_id, analysis).map_err(
                        |error| {
                            GeneratorError::ValidationError(format!(
                                "Invalid [streaming].endpoints[{index}].operation_id: {error}"
                            ))
                        },
                    )?;
                ids.extend(
                    resolution
                        .operations
                        .into_iter()
                        .map(|operation| operation.operation_id),
                );
            }
            ids
        } else {
            Default::default()
        };

        let client_prunes = self.config.enable_async_client
            && !self.config.registry_only
            && self
                .config
                .client
                .as_ref()
                .is_some_and(|client| client.prune_models);
        let server_prunes = self
            .config
            .server
            .as_ref()
            .is_some_and(|server| server.prune_models && !server.operations.is_empty());
        let extra_schema_roots = if self.config.registry_only || !self.config.enable_sse_client {
            Vec::new()
        } else {
            self.config
                .streaming_config
                .as_ref()
                .map(|streaming| {
                    streaming
                        .endpoints
                        .iter()
                        .map(|endpoint| endpoint.event_union_type.clone())
                        .collect()
                })
                .unwrap_or_default()
        };

        Ok(OperationScopes {
            client_ids,
            server_ids,
            streaming_ids,
            prune_models: client_prunes || server_prunes,
            extra_schema_roots,
        })
    }

    fn resolve_client_operation_ids(
        &self,
        analysis: &SchemaAnalysis,
    ) -> Result<Option<std::collections::BTreeSet<String>>> {
        match &self.config.client {
            Some(client) if !client.operations.is_empty() => {
                let resolution =
                    crate::server::resolve_operation_selectors(&client.operations, analysis)
                        .map_err(|error| {
                            GeneratorError::ValidationError(format!(
                                "Invalid [client].operations: {error}"
                            ))
                        })?;
                Ok(Some(
                    resolution
                        .operations
                        .into_iter()
                        .map(|operation| operation.operation_id)
                        .collect(),
                ))
            }
            _ => Ok(None),
        }
    }

    fn client_operations<'a>(
        &self,
        analysis: &'a SchemaAnalysis,
        selected: Option<&std::collections::BTreeSet<String>>,
    ) -> Vec<&'a crate::analysis::OperationInfo> {
        analysis
            .operations
            .iter()
            .filter(|(operation_id, _)| selected.is_none_or(|ids| ids.contains(*operation_id)))
            .map(|(_, operation)| operation)
            .collect()
    }

    fn prune_models_to_scopes(
        &self,
        analysis: &mut SchemaAnalysis,
        scopes: &OperationScopes,
    ) -> usize {
        if !scopes.prune_models {
            return 0;
        }

        let mut consumer_ids = scopes.server_ids.clone();
        if self.config.enable_async_client && !self.config.registry_only {
            match &scopes.client_ids {
                Some(ids) => consumer_ids.extend(ids.iter().cloned()),
                None => consumer_ids.extend(analysis.operations.keys().cloned()),
            }
        }
        consumer_ids.extend(scopes.streaming_ids.iter().cloned());

        let operations: Vec<&crate::analysis::OperationInfo> = consumer_ids
            .iter()
            .filter_map(|operation_id| analysis.operations.get(operation_id))
            .collect();
        let keep = crate::server::codegen::reachable_schemas_with_roots(
            analysis,
            &operations,
            &scopes.extra_schema_roots,
        );
        let before = analysis.schemas.len();
        analysis.schemas.retain(|name, _| keep.contains(name));
        before - analysis.schemas.len()
    }

    /// Generate HTTP error type and result alias
    fn generate_http_error_types(&self) -> TokenStream {
        quote! {
            use thiserror::Error;

            /// The generated validation-problem profile based on RFC 9457.
            /// The distinctive namespace avoids collisions with user schemas.
            pub mod openapi_to_rust_problem {
                #[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
                pub struct ProblemDetails {
                    #[serde(rename = "type")]
                    pub type_uri: String,
                    pub title: String,
                    pub status: u16,
                    pub code: String,
                    #[serde(default)]
                    pub errors: Vec<InvalidParameter>,
                    #[serde(default, skip_serializing_if = "Option::is_none")]
                    pub detail: Option<String>,
                    #[serde(default, skip_serializing_if = "Option::is_none")]
                    pub instance: Option<String>,
                }

                #[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
                pub struct InvalidParameter {
                    pub code: String,
                    pub location: String,
                    pub message: String,
                }
            }

            /// Transport-level errors: failures where we never received an
            /// inspectable HTTP response from the server.
            ///
            /// HTTP responses with non-2xx status codes are surfaced as
            /// [`ApiError`] inside [`ApiOpError::Api`], not here, so callers can
            /// always inspect status, headers, and the raw body when the server
            /// actually responded.
            #[derive(Error, Debug)]
            pub enum HttpError {
                /// Network or connection error (from reqwest)
                #[error("Network error: {0}")]
                Network(#[from] reqwest::Error),

                /// Middleware error (from reqwest-middleware)
                #[error("Middleware error: {0}")]
                Middleware(#[from] reqwest_middleware::Error),

                /// Request serialization error
                #[error("Failed to serialize request: {0}")]
                Serialization(String),

                /// Authentication error
                #[error("Authentication error: {0}")]
                Auth(String),

                /// Request timeout
                #[error("Request timeout")]
                Timeout,

                /// Invalid configuration
                #[error("Configuration error: {0}")]
                Config(String),

                /// Generic error
                #[error("{0}")]
                Other(String),
            }

            impl HttpError {
                /// Create a serialization error
                pub fn serialization_error(error: impl std::fmt::Display) -> Self {
                    Self::Serialization(error.to_string())
                }

                /// Check if this transport error is retryable
                pub fn is_retryable(&self) -> bool {
                    matches!(self, Self::Network(_) | Self::Middleware(_) | Self::Timeout)
                }
            }

            /// Envelope returned for any HTTP response that we received but
            /// couldn't (or didn't) treat as a successful typed result.
            ///
            /// Includes both non-2xx responses and 2xx responses whose body
            /// failed to deserialize into the expected success type. `status`,
            /// `headers`, and `body` are always populated so callers can
            /// inspect what the server sent without modifying the generated
            /// code. `typed` carries the parsed per-operation error variant
            /// when the body matched a declared schema. Formatting the error
            /// limits only the displayed body preview; the public fields
            /// retain the complete response and parsing details.
            #[derive(Debug, Clone)]
            pub struct ApiError<E> {
                pub status: u16,
                pub headers: reqwest::header::HeaderMap,
                pub body: String,
                pub typed: Option<E>,
                pub parse_error: Option<String>,
            }

            const API_ERROR_BODY_DISPLAY_LIMIT: usize = 500;
            const API_ERROR_BODY_TRUNCATION_MARKER: &str = "... [truncated]";

            fn display_api_error_body(body: &str) -> std::borrow::Cow<'_, str> {
                let Some((end, _)) = body.char_indices().nth(API_ERROR_BODY_DISPLAY_LIMIT) else {
                    return std::borrow::Cow::Borrowed(body);
                };

                let mut displayed =
                    String::with_capacity(end + API_ERROR_BODY_TRUNCATION_MARKER.len());
                displayed.push_str(&body[..end]);
                displayed.push_str(API_ERROR_BODY_TRUNCATION_MARKER);
                std::borrow::Cow::Owned(displayed)
            }

            impl<E> ApiError<E> {
                pub fn is_client_error(&self) -> bool {
                    (400..500).contains(&self.status)
                }

                pub fn is_server_error(&self) -> bool {
                    (500..600).contains(&self.status)
                }

                /// Retry guidance for the response. Mirrors the previous
                /// HttpError logic for backwards-compatible retry middleware.
                pub fn is_retryable(&self) -> bool {
                    matches!(self.status, 429 | 500 | 502 | 503 | 504)
                }

                /// Decode the generated RFC 9457 validation-problem profile
                /// without replacing a documented per-operation error in `typed`.
                ///
                /// Returns `None` unless the response's `Content-Type` is
                /// `application/problem+json`, which is how RFC 9457 identifies
                /// a problem document. Most third-party APIs return their
                /// errors as plain `application/json`, so this yields `None`
                /// against them by design — use `typed` for a documented
                /// per-operation error body, or `body` for the raw payload.
                /// Servers generated by this tool always emit the problem
                /// media type, so this succeeds against them.
                pub fn problem_details(
                    &self,
                ) -> Option<openapi_to_rust_problem::ProblemDetails> {
                    let content_type = self
                        .headers
                        .get(reqwest::header::CONTENT_TYPE)?
                        .to_str()
                        .ok()?;
                    let media_type = content_type.split(';').next()?.trim();
                    if !media_type.eq_ignore_ascii_case("application/problem+json") {
                        return None;
                    }
                    serde_json::from_str(&self.body).ok()
                }
            }

            impl<E: std::fmt::Debug> std::fmt::Display for ApiError<E> {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(
                        f,
                        "API error {}: {}",
                        self.status,
                        display_api_error_body(&self.body)
                    )?;

                    if let Some(typed) = &self.typed {
                        write!(f, "; typed: {typed:?}")?;
                    }

                    if let Some(parse_error) = &self.parse_error {
                        write!(f, "; parse error: {parse_error}")?;
                    }

                    Ok(())
                }
            }

            impl<E: std::fmt::Debug> std::error::Error for ApiError<E> {}

            /// Result error type returned by every generated operation method.
            ///
            /// `Transport` covers failures where we never got an inspectable
            /// response (network, timeout, middleware, request-side
            /// serialization). `Api` covers any case where the server *did*
            /// respond — the envelope always carries status + headers + raw
            /// body even when the typed deserialize fails.
            #[derive(Debug, Error)]
            pub enum ApiOpError<E: std::fmt::Debug> {
                #[error(transparent)]
                Transport(#[from] HttpError),

                #[error(transparent)]
                Api(ApiError<E>),
            }

            impl<E: std::fmt::Debug> ApiOpError<E> {
                /// Returns the API envelope when this is an `Api` variant.
                pub fn api(&self) -> Option<&ApiError<E>> {
                    match self {
                        Self::Api(e) => Some(e),
                        Self::Transport(_) => None,
                    }
                }

                /// True when the underlying error came from the server (i.e.
                /// any `Api` variant) rather than the transport layer.
                pub fn is_api_error(&self) -> bool {
                    matches!(self, Self::Api(_))
                }
            }

            // Direct From impls so `?` works without going through HttpError
            // first. Rust's `?` only chains a single `From` conversion.
            impl<E: std::fmt::Debug> From<reqwest::Error> for ApiOpError<E> {
                fn from(e: reqwest::Error) -> Self {
                    Self::Transport(HttpError::Network(e))
                }
            }

            impl<E: std::fmt::Debug> From<reqwest_middleware::Error> for ApiOpError<E> {
                fn from(e: reqwest_middleware::Error) -> Self {
                    Self::Transport(HttpError::Middleware(e))
                }
            }

            /// Result alias for transport-only error paths (e.g. helpers that
            /// don't have a per-operation error type). Generated operation
            /// methods use [`ApiOpError`] directly.
            pub type HttpResult<T> = Result<T, HttpError>;
        }
    }

    /// Generate mod.rs file that exports all modules
    fn generate_mod_file(&self, files: &[GeneratedFile]) -> Result<String> {
        let mut module_names = std::collections::BTreeSet::new();

        for file in files {
            let module_name = if file.path.components().count() > 1 {
                file.path.iter().next().and_then(|part| part.to_str())
            } else {
                file.path.file_stem().and_then(|stem| stem.to_str())
            };
            if let Some(module_name) = module_name.filter(|name| *name != "mod") {
                module_names.insert(module_name.to_string());
            }
        }
        let module_declarations = module_names
            .iter()
            .map(|name| format!("pub mod {name};"))
            .collect::<Vec<_>>();
        let pub_uses = module_names
            .iter()
            .map(|name| format!("pub use {name}::*;"))
            .collect::<Vec<_>>();

        // `module_name` is a configurable *label* — it does NOT pick
        // the on-disk directory (that's `output_dir`) and it does NOT
        // determine the Rust module path the user mounts this tree
        // at. Surfacing it in the header doc comment is the most
        // honest place: a hint to the user about what name was
        // configured and how to mount it.
        let mount_hint = format!(
            "//! Configured `module_name` = `{name}`. Mount this tree under your\n\
             //! preferred path, e.g. `pub mod {name};` in your crate root.\n",
            name = self.config.module_name,
        );
        let source_hint = self
            .source_provenance
            .as_ref()
            .map(|source| {
                format!(
                    "//! Generated by openapi-to-rust v{}. Source OpenAPI document: {source}\n",
                    env!("CARGO_PKG_VERSION")
                )
            })
            .unwrap_or_default();

        let content = format!(
            r#"//! Generated API modules
//!
//! This module exports all generated API types and clients.
//! Do not edit manually - regenerate using the appropriate script.
//!
{source_hint}
{mount_hint}
#![allow(unused_imports)]

{decls}

{uses}
"#,
            mount_hint = mount_hint,
            source_hint = source_hint,
            decls = module_declarations.join("\n"),
            uses = pub_uses.join("\n"),
        );

        Ok(content)
    }

    /// Helper method to write all generated files to disk
    pub fn output_artifacts(
        &self,
        result: &GenerationResult,
    ) -> std::collections::BTreeMap<PathBuf, String> {
        let mut artifacts = std::collections::BTreeMap::new();
        for file in &result.files {
            artifacts.insert(file.path.clone(), file.content.clone());
        }
        artifacts.insert(
            result.mod_file.path.clone(),
            result.mod_file.content.clone(),
        );
        if let Some(mut fragment) =
            crate::type_mapping::render_required_deps_toml(&result.required_deps)
        {
            if let Some(source) = &self.source_provenance {
                let header = format!(
                    "# Generated by openapi-to-rust v{}. Source OpenAPI document: {source}",
                    env!("CARGO_PKG_VERSION")
                );
                fragment = fragment.replacen("# Generated by openapi-to-rust.", &header, 1);
            }
            artifacts.insert(PathBuf::from("REQUIRED_DEPS.toml"), fragment);
        }
        artifacts
    }

    /// Write a generation result using the same rendered artifact set exposed
    /// to dry-run and check-mode callers.
    pub fn write_files(&self, result: &GenerationResult) -> Result<()> {
        use std::fs;

        // Create output directory if it doesn't exist
        fs::create_dir_all(&self.config.output_dir)?;

        let artifacts = self.output_artifacts(result);
        for (relative, content) in &artifacts {
            let file_path = self.config.output_dir.join(relative);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file_path, content)?;
        }

        let deps_path = self.config.output_dir.join("REQUIRED_DEPS.toml");
        if !artifacts.contains_key(std::path::Path::new("REQUIRED_DEPS.toml")) && deps_path.exists()
        {
            fs::remove_file(&deps_path)?;
        }

        Ok(())
    }

    fn generate_type_definition(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        analysis: &crate::analysis::SchemaAnalysis,
        type_context: &TypeGenerationContext<'_>,
    ) -> Result<TokenStream> {
        use crate::analysis::SchemaType;

        match &schema.schema_type {
            SchemaType::Primitive { rust_type, .. } => {
                // Generate type alias for primitives that are referenced by other schemas
                self.generate_type_alias(schema, rust_type)
            }
            SchemaType::StringEnum { values } => {
                let ext = analysis.enum_extensions.get(&schema.name);
                // [extensible_enums] override: opt a closed string-enum into an
                // extensible enum when the spec is known to lag the API (e.g.
                // Cloudflare R2 returning "WNAM" against a lowercase-only enum).
                // Accept either the raw spec name (e.g. "r2_bucket_location")
                // or the rendered Rust type name (e.g. "R2BucketLocation") so
                // users can write whichever they see in the generated code.
                let rust_name = self.to_rust_type_name(&schema.name);
                let force_extensible = self
                    .config
                    .extensible_enum_overrides
                    .get(&schema.name)
                    .or_else(|| self.config.extensible_enum_overrides.get(&rust_name))
                    .copied()
                    .unwrap_or(false);
                if force_extensible {
                    self.generate_extensible_enum(schema, values, ext)
                } else {
                    self.generate_string_enum(schema, values, ext)
                }
            }
            SchemaType::ExtensibleEnum { known_values } => {
                let ext = analysis.enum_extensions.get(&schema.name);
                self.generate_extensible_enum(schema, known_values, ext)
            }
            SchemaType::Object {
                properties,
                required,
                additional_properties,
            } => self.generate_struct(
                schema,
                properties,
                required,
                additional_properties,
                analysis,
                type_context,
            ),
            SchemaType::DiscriminatedUnion {
                discriminator_field,
                variants,
            } => {
                // Check if this discriminated union should be untagged due to being nested
                if self.should_use_untagged_discriminated_union(schema, analysis) {
                    // Convert variants to SchemaRef format for union enum generation
                    let schema_refs: Vec<crate::analysis::SchemaRef> = variants
                        .iter()
                        .map(|v| crate::analysis::SchemaRef {
                            target: v.type_name.clone(),
                            nullable: false,
                        })
                        .collect();
                    self.generate_union_enum(schema, &schema_refs, analysis)
                } else {
                    self.generate_discriminated_enum(
                        schema,
                        discriminator_field,
                        variants,
                        analysis,
                    )
                }
            }
            SchemaType::Union { variants } => self.generate_union_enum(schema, variants, analysis),
            SchemaType::Reference { target } => {
                // For references, check if we need to generate a type alias
                // This handles cases like nullable patterns
                if schema.name != *target {
                    // Generate a type alias
                    let alias_name = format_ident!("{}", self.to_rust_type_name(&schema.name));
                    let target_type = format_ident!("{}", self.to_rust_type_name(target));

                    let doc_comment = if let Some(desc) = &schema.description {
                        quote! { #[doc = #desc] }
                    } else {
                        TokenStream::new()
                    };

                    Ok(quote! {
                        #doc_comment
                        pub type #alias_name = #target_type;
                    })
                } else {
                    // Same name as target, no need for alias
                    Ok(TokenStream::new())
                }
            }
            SchemaType::Array { item_type } => {
                // Generate type alias for named array schemas.
                //
                // Special case: if the array item is a struct whose discriminator
                // field was stripped (because it's used in a tagged enum), the bare
                // struct won't serialize the discriminator in standalone contexts.
                // Generate a single-variant tagged wrapper enum so the discriminator
                // field is re-added by serde's tag attribute.
                let array_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

                // Check if the item type is a Reference to a discriminator-stripped struct
                if let SchemaType::Reference { target } = item_type.as_ref() {
                    if let Some(info) = type_context.discriminated_variants.get(target) {
                        if !info.is_parent_untagged {
                            // Generate a wrapper enum that re-adds the discriminator tag
                            let wrapper_name =
                                format_ident!("{}Item", self.to_rust_type_name(&schema.name));
                            let variant_type = format_ident!("{}", self.to_rust_type_name(target));
                            let disc_field = &info.discriminator_field;
                            let disc_value = &info.discriminator_value;

                            let doc_comment = if let Some(desc) = &schema.description {
                                quote! { #[doc = #desc] }
                            } else {
                                TokenStream::new()
                            };

                            return Ok(quote! {
                                /// Wrapper enum that re-adds the discriminator tag
                                /// for array contexts where the inner struct had its
                                /// discriminator field stripped for tagged enum use.
                                #[derive(Debug, Clone, Deserialize, Serialize)]
                                #[serde(tag = #disc_field)]
                                pub enum #wrapper_name {
                                    #[serde(rename = #disc_value)]
                                    #variant_type(#variant_type),
                                }
                                #doc_comment
                                pub type #array_name = Vec<#wrapper_name>;
                            });
                        }
                    }
                }

                let inner_type = self.generate_array_item_type(item_type, analysis);

                let doc_comment = if let Some(desc) = &schema.description {
                    quote! { #[doc = #desc] }
                } else {
                    TokenStream::new()
                };

                Ok(quote! {
                    #doc_comment
                    pub type #array_name = Vec<#inner_type>;
                })
            }
            SchemaType::Composition { schemas } => {
                self.generate_composition_struct(schema, schemas)
            }
        }
    }

    fn generate_type_alias(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        rust_type: &str,
    ) -> Result<TokenStream> {
        let type_name = format_ident!("{}", self.to_rust_type_name(&schema.name));
        // syn parses any valid Rust type expression including
        // generics (`chrono::DateTime<chrono::Utc>`, `Vec<u8>`).
        // The pre-Q2 ad-hoc `::`-splitter choked on `<`.
        let base_type = parse_rust_type(rust_type)?;

        let doc_comment = if let Some(desc) = &schema.description {
            let sanitized_desc = self.sanitize_doc_comment(desc);
            quote! { #[doc = #sanitized_desc] }
        } else {
            TokenStream::new()
        };

        Ok(quote! {
            #doc_comment
            pub type #type_name = #base_type;
        })
    }

    fn generate_extensible_enum(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        known_values: &[String],
        ext: Option<&crate::analysis::EnumExtensions>,
    ) -> Result<TokenStream> {
        let enum_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // Q2.6: pre-resolve variant idents from x-enum-varnames when
        // available + length-matched + toggle on. Same fallback rule
        // as generate_string_enum.
        let varnames_override: Option<&Vec<String>> = ext
            .filter(|_| self.config.types.x_enum_varnames_enabled())
            .map(|e| &e.varnames)
            .filter(|v| !v.is_empty() && v.len() == known_values.len());
        let descriptions_override: Option<&Vec<String>> = ext
            .filter(|_| self.config.types.x_enum_descriptions_enabled())
            .map(|e| &e.descriptions)
            .filter(|v| !v.is_empty() && v.len() == known_values.len());

        let variant_ident_for = |index: usize, value: &str| -> proc_macro2::Ident {
            let name = match varnames_override {
                Some(v) => v[index].clone(),
                None => self.to_rust_enum_variant(value),
            };
            format_ident!("{}", name)
        };

        // For extensible enums, we need a different approach:
        // 1. Create a regular enum with known variants + Custom
        // 2. Implement custom serialization/deserialization

        let known_variants = known_values.iter().enumerate().map(|(i, value)| {
            let variant_ident = variant_ident_for(i, value);
            let doc = descriptions_override
                .map(|d| {
                    let s = self.sanitize_doc_comment(&d[i]);
                    quote! { #[doc = #s] }
                })
                .unwrap_or_default();
            quote! {
                #doc
                #variant_ident,
            }
        });

        let match_arms_de = known_values.iter().enumerate().map(|(i, value)| {
            let variant_ident = variant_ident_for(i, value);
            quote! {
                #value => Ok(#enum_name::#variant_ident),
            }
        });

        let match_arms_ser = known_values.iter().enumerate().map(|(i, value)| {
            let variant_ident = variant_ident_for(i, value);
            quote! {
                #enum_name::#variant_ident => #value,
            }
        });

        let derives = if self.config.enable_specta {
            quote! {
                #[derive(Debug, Clone, PartialEq, Eq)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
            }
        } else {
            quote! {
                #[derive(Debug, Clone, PartialEq, Eq)]
            }
        };

        Ok(quote! {
            #doc_comment
            #derives
            pub enum #enum_name {
                #(#known_variants)*
                /// Custom or unknown model identifier
                Custom(String),
            }

            impl<'de> serde::Deserialize<'de> for #enum_name {
                fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
                where
                    D: serde::Deserializer<'de>,
                {
                    let value = String::deserialize(deserializer)?;
                    match value.as_str() {
                        #(#match_arms_de)*
                        _ => Ok(#enum_name::Custom(value)),
                    }
                }
            }

            impl serde::Serialize for #enum_name {
                fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
                where
                    S: serde::Serializer,
                {
                    let value = match self {
                        #(#match_arms_ser)*
                        #enum_name::Custom(s) => s.as_str(),
                    };
                    serializer.serialize_str(value)
                }
            }
        })
    }

    fn generate_string_enum(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        values: &[String],
        ext: Option<&crate::analysis::EnumExtensions>,
    ) -> Result<TokenStream> {
        let enum_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        // Determine which variant should be the default. The spec's `default`
        // may not exactly match any enum value (telnyx has
        // `default: "en"` on a language enum that lists `en-US`, `en-AU`,
        // … — no exact match). When that happens, drop the `Default` derive
        // entirely instead of emitting it on an enum where no variant has
        // `#[default]` (E0665).
        let default_value = schema
            .default
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let has_default_match = match &default_value {
            Some(d) => values.iter().any(|v| v == d),
            None => !values.is_empty(),
        };

        // Q2.6: x-enum-varnames overrides the default heuristic when
        // present, length-matched, and the toggle is on. Falls back
        // to the to_rust_enum_variant heuristic otherwise.
        let varnames_override: Option<&Vec<String>> = ext
            .filter(|_| self.config.types.x_enum_varnames_enabled())
            .map(|e| &e.varnames)
            .filter(|v| !v.is_empty() && v.len() == values.len());
        let descriptions_override: Option<&Vec<String>> = ext
            .filter(|_| self.config.types.x_enum_descriptions_enabled())
            .map(|e| &e.descriptions)
            .filter(|v| !v.is_empty() && v.len() == values.len());

        // Variant-name uniqueness: enum values that PascalCase to the same
        // identifier (e.g. `ASC`/`asc` both → `Asc`) collide and produce
        // E0428 + non-exhaustive matches downstream. Dedupe by suffixing
        // `_2`, `_3`, … on collisions while preserving the first occurrence's
        // name, and keeping each variant's `#[serde(rename)]` pointed at the
        // original wire string.
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        let variant_pairs: Vec<(syn::Ident, &String, bool, Option<String>)> = values
            .iter()
            .enumerate()
            .map(|(i, value)| {
                let base = match varnames_override {
                    Some(v) => v[i].clone(),
                    None => self.to_rust_enum_variant(value),
                };
                let mut variant_name = base.clone();
                let mut suffix = 2;
                while !used.insert(variant_name.clone()) {
                    variant_name = format!("{base}_{suffix}");
                    suffix += 1;
                }
                let variant_ident = format_ident!("{}", variant_name);
                let is_default = if let Some(ref default) = default_value {
                    value == default
                } else {
                    i == 0
                };
                let description = descriptions_override.map(|d| d[i].clone());
                (variant_ident, value, is_default, description)
            })
            .collect();

        let variants =
            variant_pairs
                .iter()
                .map(|(variant_ident, value, is_default, description)| {
                    let doc = description
                        .as_ref()
                        .map(|d| {
                            let s = self.sanitize_doc_comment(d);
                            quote! { #[doc = #s] }
                        })
                        .unwrap_or_default();
                    if *is_default {
                        quote! {
                            #doc
                            #[default]
                            #[serde(rename = #value)]
                            #variant_ident,
                        }
                    } else {
                        quote! {
                            #doc
                            #[serde(rename = #value)]
                            #variant_ident,
                        }
                    }
                });

        // T13/T10: emit `as_str` and `Display` so the enum can be embedded in
        // query strings, headers, and path segments without requiring callers
        // to reach for `serde_json` round-trips.
        let as_str_arms = variant_pairs.iter().map(|(variant_ident, value, _, _)| {
            quote! { Self::#variant_ident => #value, }
        });

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // Generate derives with optional Specta support. Drop `Default` if
        // no variant ends up tagged `#[default]` (would trigger E0665).
        let derives = match (self.config.enable_specta, has_default_match) {
            (true, true) => quote! {
                #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
            },
            (true, false) => quote! {
                #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
            },
            (false, true) => quote! {
                #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
            },
            (false, false) => quote! {
                #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
            },
        };

        Ok(quote! {
            #doc_comment
            #derives
            pub enum #enum_name {
                #(#variants)*
            }

            impl #enum_name {
                pub fn as_str(&self) -> &'static str {
                    match self {
                        #(#as_str_arms)*
                    }
                }
            }

            impl ::std::fmt::Display for #enum_name {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    f.write_str(self.as_str())
                }
            }

            impl AsRef<str> for #enum_name {
                fn as_ref(&self) -> &str {
                    self.as_str()
                }
            }
        })
    }

    fn generate_struct(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        properties: &BTreeMap<String, crate::analysis::PropertyInfo>,
        required: &std::collections::HashSet<String>,
        additional_properties: &crate::analysis::ObjectAdditionalProperties,
        analysis: &crate::analysis::SchemaAnalysis,
        type_context: &TypeGenerationContext<'_>,
    ) -> Result<TokenStream> {
        let struct_name = format_ident!("{}", self.to_rust_type_name(&schema.name));
        let emitted_properties = self.emitted_object_properties(
            &schema.name,
            properties,
            required,
            additional_properties,
            analysis,
            type_context.discriminated_variants.get(&schema.name),
        );

        let mut fields: Vec<TokenStream> = emitted_properties
            .iter()
            .map(|emitted| {
                let field_name = emitted.wire_name;
                let property = emitted.property;
                let field_ident = &emitted.ident;
                let field_type = &emitted.field_type;
                let serde_attrs = self.generate_serde_field_attrs(
                    &schema.name,
                    field_name,
                    field_ident,
                    property,
                    emitted.is_required,
                    analysis,
                );
                let specta_attrs = self.generate_specta_field_attrs(field_name);

                let doc_comment = if let Some(desc) = &property.description {
                    let sanitized_desc = self.sanitize_doc_comment(desc);
                    quote! { #[doc = #sanitized_desc] }
                } else {
                    TokenStream::new()
                };
                let constraint_doc = self.generate_constraint_doc(&property.constraints);

                quote! {
                    #doc_comment
                    #constraint_doc
                    #serde_attrs
                    #specta_attrs
                    pub #field_ident: #field_type,
                }
            })
            .collect();

        // Q2.3: emit the catch-all additional-properties field with
        // the right value type. `Untyped` keeps pre-Q2.3 behavior
        // (BTreeMap<String, serde_json::Value>); `Typed { value_type }`
        // surfaces the actual schema-declared type, e.g.
        // BTreeMap<String, MyValue>. `Forbidden` emits no field.
        match additional_properties {
            crate::analysis::ObjectAdditionalProperties::Forbidden => {}
            crate::analysis::ObjectAdditionalProperties::Untyped => {
                fields.push(quote! {
                    /// Additional properties not explicitly defined in the schema
                    #[serde(flatten)]
                    pub additional_properties:
                        std::collections::BTreeMap<String, serde_json::Value>,
                });
            }
            crate::analysis::ObjectAdditionalProperties::Typed { value_type } => {
                let value_tokens = self.generate_array_item_type(value_type, analysis);
                fields.push(quote! {
                    /// Additional properties matching the spec's
                    /// `additionalProperties` value schema.
                    #[serde(flatten)]
                    pub additional_properties:
                        std::collections::BTreeMap<String, #value_tokens>,
                });
            }
        }

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // Default is safe only when no emitted wire property is required.
        // Optional fields are represented as Option<T>, and the generated
        // additional-properties map (when present) is empty by default. We do
        // not invent values for required data, even when the Rust type itself
        // happens to implement Default.
        let can_derive_default = emitted_properties
            .iter()
            .all(|property| !property.is_required);

        // Generate derives with optional Specta support
        // Note: We use snake_case everywhere (matching the OpenAPI spec) for consistency
        // between Rust, JSON API, and TypeScript
        let derives = match (self.config.enable_specta, can_derive_default) {
            (true, true) => quote! {
                #[derive(Debug, Clone, Deserialize, Serialize, Default)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
            },
            (true, false) => quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
            },
            (false, true) => quote! {
                #[derive(Debug, Clone, Deserialize, Serialize, Default)]
            },
            (false, false) => quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
            },
        };

        let builder = if type_context.index.request_body_roots.contains(&schema.name)
            && emitted_properties
                .iter()
                .any(|property| property.is_required)
            && (emitted_properties
                .iter()
                .any(|property| !property.is_required)
                || !matches!(
                    additional_properties,
                    crate::analysis::ObjectAdditionalProperties::Forbidden
                )) {
            self.generate_request_model_builder(
                schema,
                &emitted_properties,
                additional_properties,
                analysis,
                type_context.index,
            )
        } else {
            TokenStream::new()
        };

        Ok(quote! {
            #doc_comment
            #derives
            pub struct #struct_name {
                #(#fields)*
            }

            #builder
        })
    }

    /// Project an object schema into the exact public fields emitted in
    /// `types.rs`. Request-model and operation builders share this metadata so
    /// identifier disambiguation, discriminator filtering, and Option wrapping
    /// cannot drift.
    pub(crate) fn emitted_object_properties<'a>(
        &self,
        schema_name: &str,
        properties: &'a BTreeMap<String, crate::analysis::PropertyInfo>,
        required: &std::collections::HashSet<String>,
        additional_properties: &crate::analysis::ObjectAdditionalProperties,
        analysis: &crate::analysis::SchemaAnalysis,
        discriminator_info: Option<&DiscriminatedVariantInfo>,
    ) -> Vec<EmittedObjectProperty<'a>> {
        let mut sorted_properties: Vec<_> = properties.iter().collect();
        sorted_properties.sort_by_key(|(name, _)| name.as_str());

        let mut used_field_idents = std::collections::HashSet::new();
        if !matches!(
            additional_properties,
            crate::analysis::ObjectAdditionalProperties::Forbidden
        ) {
            used_field_idents.insert("additional_properties".to_string());
        }

        let mut emitted = Vec::new();
        for (field_name, property) in sorted_properties {
            if discriminator_info.is_some_and(|info| {
                !info.is_parent_untagged && field_name.as_str() == info.discriminator_field.as_str()
            }) {
                continue;
            }

            let raw = self.to_rust_field_name(field_name);
            let mut chosen = raw.clone();
            let mut suffix = 2;
            while !used_field_idents.insert(chosen.clone()) {
                chosen = format!("{raw}_{suffix}");
                suffix += 1;
            }
            let is_required = required.contains(field_name);
            emitted.push(EmittedObjectProperty {
                wire_name: field_name,
                property,
                ident: Self::to_field_ident(&chosen),
                is_required,
                field_type: self.generate_field_type(
                    schema_name,
                    field_name,
                    property,
                    is_required,
                    analysis,
                ),
            });
        }
        emitted
    }

    fn type_generation_index(
        &self,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> TypeGenerationIndex {
        let reserved_type_names = analysis
            .schemas
            .keys()
            .map(|name| self.to_rust_type_name(name))
            .collect();
        let mut request_body_roots = std::collections::HashSet::new();
        for operation in analysis.operations.values() {
            let Some(mut current) = operation
                .request_body
                .as_ref()
                .and_then(crate::analysis::RequestBodyContent::schema_name)
            else {
                continue;
            };
            while request_body_roots.insert(current.to_string()) {
                let Some(crate::analysis::AnalyzedSchema {
                    schema_type: crate::analysis::SchemaType::Reference { target },
                    ..
                }) = analysis.schemas.get(current)
                else {
                    break;
                };
                current = target;
            }
        }
        TypeGenerationIndex {
            request_body_roots,
            reserved_type_names,
        }
    }

    fn generate_request_model_builder(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        properties: &[EmittedObjectProperty<'_>],
        additional_properties: &crate::analysis::ObjectAdditionalProperties,
        analysis: &crate::analysis::SchemaAnalysis,
        type_index: &TypeGenerationIndex,
    ) -> TokenStream {
        let struct_name = format_ident!("{}", self.to_rust_type_name(&schema.name));
        let builder_base = format!("{}Builder", struct_name);
        let mut builder_name = builder_base.clone();
        let mut suffix = 2;
        while type_index.reserved_type_names.contains(&builder_name) {
            builder_name = format!("{builder_base}{suffix}");
            suffix += 1;
        }
        let builder_name = format_ident!("{builder_name}");

        let required_parameters: Vec<TokenStream> = properties
            .iter()
            .filter(|property| property.is_required)
            .map(|property| {
                let ident = &property.ident;
                let field_type = &property.field_type;
                quote! { #ident: #field_type }
            })
            .collect();
        let required_idents: Vec<&syn::Ident> = properties
            .iter()
            .filter(|property| property.is_required)
            .map(|property| &property.ident)
            .collect();
        let optional_initializers: Vec<TokenStream> = properties
            .iter()
            .filter(|property| !property.is_required)
            .map(|property| {
                let ident = &property.ident;
                quote! { #ident: None }
            })
            .collect();

        let additional_initializer = match additional_properties {
            crate::analysis::ObjectAdditionalProperties::Forbidden => TokenStream::new(),
            crate::analysis::ObjectAdditionalProperties::Untyped
            | crate::analysis::ObjectAdditionalProperties::Typed { .. } => quote! {
                additional_properties: ::std::collections::BTreeMap::new(),
            },
        };

        let mut used_builder_methods =
            std::collections::HashSet::from(["new".to_string(), "build".to_string()]);
        if !matches!(
            additional_properties,
            crate::analysis::ObjectAdditionalProperties::Forbidden
        ) {
            used_builder_methods.insert("additional_properties".to_string());
        }
        let optional_setters: Vec<TokenStream> = properties
            .iter()
            .filter(|property| !property.is_required)
            .map(|property| {
                let field_ident = &property.ident;
                let field_type = self.generate_property_base_type(
                    &schema.name,
                    property.wire_name,
                    property.property,
                    analysis,
                );
                // Allocate every setter in the builder's method namespace.
                // `new` and `build` keep their documented `with_` escape;
                // further collisions receive deterministic numeric suffixes.
                let field_name = field_ident.to_string();
                let plain_field_name = field_name.strip_prefix("r#").unwrap_or(&field_name);
                let mut setter_name = if matches!(plain_field_name, "new" | "build") {
                    format!("with_{plain_field_name}")
                } else {
                    field_name.clone()
                };
                let setter_base = setter_name.clone();
                let mut suffix = 2;
                while !used_builder_methods.insert(setter_name.clone()) {
                    setter_name = format!("{setter_base}_{suffix}");
                    suffix += 1;
                }
                let setter_ident = Self::to_field_ident(&setter_name);
                let wire_name = property.wire_name;
                quote! {
                    #[doc = concat!("Set the optional `", #wire_name, "` request field.")]
                    #[must_use]
                    pub fn #setter_ident(mut self, #field_ident: #field_type) -> Self {
                        self.value.#field_ident = Some(#field_ident);
                        self
                    }
                }
            })
            .collect();

        let additional_setter = match additional_properties {
            crate::analysis::ObjectAdditionalProperties::Forbidden => TokenStream::new(),
            crate::analysis::ObjectAdditionalProperties::Untyped => quote! {
                /// Replace the request's additional properties.
                #[must_use]
                pub fn additional_properties(
                    mut self,
                    additional_properties: ::std::collections::BTreeMap<
                        String,
                        serde_json::Value,
                    >,
                ) -> Self {
                    self.value.additional_properties = additional_properties;
                    self
                }
            },
            crate::analysis::ObjectAdditionalProperties::Typed { value_type } => {
                let value_type = self.generate_array_item_type(value_type, analysis);
                quote! {
                    /// Replace the request's additional properties.
                    #[must_use]
                    pub fn additional_properties(
                        mut self,
                        additional_properties: ::std::collections::BTreeMap<
                            String,
                            #value_type,
                        >,
                    ) -> Self {
                        self.value.additional_properties = additional_properties;
                        self
                    }
                }
            }
        };

        quote! {
            impl #struct_name {
                /// Construct this request with every required wire field.
                pub fn new(#(#required_parameters),*) -> Self {
                    Self {
                        #(#required_idents,)*
                        #(#optional_initializers,)*
                        #additional_initializer
                    }
                }

                /// Start a dependency-free builder with every required wire field.
                pub fn builder(#(#required_parameters),*) -> #builder_name {
                    #builder_name::new(#(#required_idents),*)
                }
            }

            /// Dependency-free builder for [`#struct_name`].
            #[derive(Debug, Clone)]
            #[must_use]
            pub struct #builder_name {
                value: #struct_name,
            }

            impl #builder_name {
                /// Start a builder with every required wire field.
                pub fn new(#(#required_parameters),*) -> Self {
                    Self {
                        value: #struct_name::new(#(#required_idents),*),
                    }
                }

                #(#optional_setters)*
                #additional_setter

                /// Finish building the request model.
                pub fn build(self) -> #struct_name {
                    self.value
                }
            }
        }
    }

    fn generate_discriminated_enum(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        discriminator_field: &str,
        variants: &[crate::analysis::UnionVariant],
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> Result<TokenStream> {
        let enum_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        // Check if any variant references another discriminated union
        let has_nested_discriminated_union = variants.iter().any(|variant| {
            if let Some(variant_schema) = analysis.schemas.get(&variant.type_name) {
                matches!(
                    variant_schema.schema_type,
                    crate::analysis::SchemaType::DiscriminatedUnion { .. }
                )
            } else {
                false
            }
        });

        // If we have a nested discriminated union, make this enum untagged
        if has_nested_discriminated_union {
            // Generate as untagged union
            let schema_refs: Vec<crate::analysis::SchemaRef> = variants
                .iter()
                .map(|v| crate::analysis::SchemaRef {
                    target: v.type_name.clone(),
                    nullable: false,
                })
                .collect();
            return self.generate_union_enum(schema, &schema_refs, analysis);
        }

        let enclosing = self.to_rust_type_name(&schema.name);
        let enum_variants = variants.iter().map(|variant| {
            let variant_name = format_ident!("{}", variant.rust_name);
            let variant_value = &variant.discriminator_value;

            let variant_type = format_ident!("{}", self.to_rust_type_name(&variant.type_name));
            // Box variant payloads that point at the enclosing enum or any
            // schema in the analysis's recursive set, otherwise the enum has
            // infinite size (E0072).
            let payload = if self.to_rust_type_name(&variant.type_name) == enclosing
                || analysis
                    .dependencies
                    .recursive_schemas
                    .contains(&variant.type_name)
            {
                quote! { Box<#variant_type> }
            } else {
                quote! { #variant_type }
            };
            quote! {
                #[serde(rename = #variant_value)]
                #variant_name(#payload),
            }
        });

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // Generate derives with optional Specta support
        let derives = if self.config.enable_specta {
            quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
                #[serde(tag = #discriminator_field)]
            }
        } else {
            quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
                #[serde(tag = #discriminator_field)]
            }
        };

        Ok(quote! {
            #doc_comment
            #derives
            pub enum #enum_name {
                #(#enum_variants)*
            }
        })
    }

    /// Check if a discriminated union should be generated as untagged due to being nested
    fn should_use_untagged_discriminated_union(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> bool {
        // Only make discriminated unions untagged if they are nested AND their variants
        // don't need the discriminator field for API compatibility

        // Check if this schema is used as a variant in another discriminated union
        for other_schema in analysis.schemas.values() {
            if let crate::analysis::SchemaType::DiscriminatedUnion {
                variants,
                discriminator_field: _,
            } = &other_schema.schema_type
            {
                for variant in variants {
                    if variant.type_name == schema.name {
                        // This discriminated union is nested inside another discriminated union

                        // Check if the current schema's variants have the discriminator field in their properties
                        // If they do, we need to keep this union tagged to preserve the discriminator
                        if let crate::analysis::SchemaType::DiscriminatedUnion {
                            discriminator_field: current_discriminator,
                            variants: current_variants,
                            ..
                        } = &schema.schema_type
                        {
                            // Check if any variant schemas have the discriminator field as a property
                            for current_variant in current_variants {
                                if let Some(variant_schema) =
                                    analysis.schemas.get(&current_variant.type_name)
                                {
                                    if let crate::analysis::SchemaType::Object {
                                        properties, ..
                                    } = &variant_schema.schema_type
                                    {
                                        if properties.contains_key(current_discriminator) {
                                            // This variant has the discriminator field as a property,
                                            // so we need to keep the union tagged to preserve it
                                            return false;
                                        }
                                    }
                                }
                            }
                        }

                        // No variants have the discriminator as a property, safe to make untagged
                        return true;
                    }
                }
            }
        }
        false
    }

    fn generate_union_enum(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        variants: &[crate::analysis::SchemaRef],
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> Result<TokenStream> {
        let enum_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        // Generate meaningful variant names based on type names
        let mut used_variant_names = std::collections::HashSet::new();
        let enum_variants = variants.iter().enumerate().map(|(i, variant)| {
            // Generate a meaningful variant name from the type name
            let base_variant_name = self.type_name_to_variant_name(&variant.target);
            let variant_name = self.ensure_unique_variant_name_generator(
                base_variant_name,
                &mut used_variant_names,
                i,
            );
            let variant_name_ident = format_ident!("{}", variant_name);

            // For primitive types and Vec types, use them directly without conversion
            let variant_type_tokens = if matches!(
                variant.target.as_str(),
                "bool"
                    | "i8"
                    | "i16"
                    | "i32"
                    | "i64"
                    | "i128"
                    | "u8"
                    | "u16"
                    | "u32"
                    | "u64"
                    | "u128"
                    | "f32"
                    | "f64"
                    | "String"
            ) {
                let type_ident = format_ident!("{}", variant.target);
                quote! { #type_ident }
            } else if variant.target == "serde_json::Value" {
                // The target is a fully-qualified path; emit it as a path so
                // it doesn't get mangled into a phantom `SerdeJsonValue` ident.
                quote! { serde_json::Value }
            } else if variant.target.starts_with("Vec<") && variant.target.ends_with(">") {
                // Handle Vec types by parsing the inner type
                let inner = &variant.target[4..variant.target.len() - 1];

                // Handle nested Vec types (e.g., Vec<Vec<i64>>)
                if inner.starts_with("Vec<") && inner.ends_with(">") {
                    let inner_inner = &inner[4..inner.len() - 1];
                    if inner_inner == "serde_json::Value" {
                        quote! { Vec<Vec<serde_json::Value>> }
                    } else {
                        let inner_inner_type = if matches!(
                            inner_inner,
                            "bool"
                                | "i8"
                                | "i16"
                                | "i32"
                                | "i64"
                                | "i128"
                                | "u8"
                                | "u16"
                                | "u32"
                                | "u64"
                                | "u128"
                                | "f32"
                                | "f64"
                                | "String"
                        ) {
                            format_ident!("{}", inner_inner)
                        } else {
                            format_ident!("{}", self.to_rust_type_name(inner_inner))
                        };
                        quote! { Vec<Vec<#inner_inner_type>> }
                    }
                } else if inner == "serde_json::Value" {
                    quote! { Vec<serde_json::Value> }
                } else {
                    let inner_type = if matches!(
                        inner,
                        "bool"
                            | "i8"
                            | "i16"
                            | "i32"
                            | "i64"
                            | "i128"
                            | "u8"
                            | "u16"
                            | "u32"
                            | "u64"
                            | "u128"
                            | "f32"
                            | "f64"
                            | "String"
                    ) {
                        format_ident!("{}", inner)
                    } else {
                        format_ident!("{}", self.to_rust_type_name(inner))
                    };
                    quote! { Vec<#inner_type> }
                }
            } else if variant.target.contains("::") || variant.target.contains('<') {
                // Qualified Rust path or generic (chrono::DateTime<chrono::Utc>,
                // bytes::Bytes, std::net::Ipv4Addr) emitted by TypeMapper. Pass
                // it straight to syn — the to_rust_type_name PascalCase
                // pipeline below would mangle it into a non-existent ident.
                parse_rust_type(&variant.target).unwrap_or_else(|_| {
                    let fallback = format_ident!("{}", self.to_rust_type_name(&variant.target));
                    quote! { #fallback }
                })
            } else {
                let type_ident = format_ident!("{}", self.to_rust_type_name(&variant.target));
                quote! { #type_ident }
            };

            // Self-referential variant (variant payload type == enclosing
            // enum) yields an infinite-size enum (E0072). Wrap in `Box<T>` to
            // break the cycle. Observed in microsoft-graph.yaml.
            let target_rust_name = self.to_rust_type_name(&variant.target);
            let enclosing_name = self.to_rust_type_name(&schema.name);
            let is_self_ref = target_rust_name == enclosing_name;
            // Indirect cycles (stripe BankAccount → BankAccountCustomer →
            // Customer → BankAccountCustomer): variants pointing into the
            // analysis's recursive_schemas set must also be heap-allocated.
            let is_recursive_target = analysis
                .dependencies
                .recursive_schemas
                .contains(&variant.target);
            let variant_type_tokens = if is_self_ref || is_recursive_target {
                quote! { Box<#variant_type_tokens> }
            } else {
                variant_type_tokens
            };

            quote! {
                #variant_name_ident(#variant_type_tokens),
            }
        });

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // Generate derives with optional Specta support
        let derives = if self.config.enable_specta {
            quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
                #[serde(untagged)]
            }
        } else {
            quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
                #[serde(untagged)]
            }
        };

        Ok(quote! {
            #doc_comment
            #derives
            pub enum #enum_name {
                #(#enum_variants)*
            }
        })
    }

    /// Walk a chain of type-alias `Reference`s starting from `target` and
    /// return true if the chain reaches the schema named by
    /// `enclosing_rust_name` (Rust name). Bounded depth to prevent infinite
    /// loops on truly cyclic aliases.
    fn target_aliases_back_to(
        &self,
        target: &str,
        enclosing_rust_name: &str,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> bool {
        let mut current = target.to_string();
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        for _ in 0..16 {
            if !visited.insert(current.clone()) {
                return true;
            }
            let Some(schema) = analysis.schemas.get(&current) else {
                return false;
            };
            if let crate::analysis::SchemaType::Reference { target: next } = &schema.schema_type {
                if self.to_rust_type_name(next) == enclosing_rust_name {
                    return true;
                }
                current = next.clone();
                continue;
            }
            return false;
        }
        false
    }

    fn generate_field_type(
        &self,
        schema_name: &str,
        field_name: &str,
        prop: &crate::analysis::PropertyInfo,
        is_required: bool,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> TokenStream {
        let base_type = self.generate_property_base_type(schema_name, field_name, prop, analysis);

        if self.property_is_option_wrapped(schema_name, field_name, prop, is_required, analysis) {
            quote! { Option<#base_type> }
        } else {
            base_type
        }
    }

    fn property_is_option_wrapped(
        &self,
        schema_name: &str,
        field_name: &str,
        prop: &crate::analysis::PropertyInfo,
        is_required: bool,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> bool {
        let override_key = format!("{schema_name}.{field_name}");
        let is_nullable_override = self
            .config
            .nullable_field_overrides
            .get(&override_key)
            .copied()
            .unwrap_or(false);

        !is_required
            || prop.nullable
            || is_nullable_override
            || (prop.default.is_some() && self.type_lacks_default(&prop.schema_type, analysis))
    }

    pub(crate) fn generate_property_base_type(
        &self,
        schema_name: &str,
        _field_name: &str,
        prop: &crate::analysis::PropertyInfo,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> TokenStream {
        use crate::analysis::SchemaType;

        match &prop.schema_type {
            SchemaType::Primitive { rust_type, .. } => {
                // syn handles generics + complex paths
                // (chrono::DateTime<chrono::Utc>, Vec<u8>, …).
                parse_rust_type(rust_type).unwrap_or_else(|_| {
                    // Pathological mapper output: fall back to bare
                    // String so the generated file at least
                    // compiles. Emit a stderr warning so the
                    // operator can investigate.
                    eprintln!(
                        "⚠️  TypeMapper produced un-parseable type `{rust_type}`; \
                         falling back to String"
                    );
                    quote! { String }
                })
            }
            SchemaType::Reference { target } => {
                let target_rust_name = self.to_rust_type_name(target);
                let target_type = format_ident!("{}", target_rust_name);
                // Wrap recursive references in Box<T> for heap allocation.
                // Three ways to detect the cycle:
                // 1. Target is in the analysis-level recursive set (catches
                //    direct + indirect cycles via the dependency graph).
                // 2. Target's Rust name equals the enclosing struct's Rust
                //    name (catches cloudflare-style cases where two distinct
                //    spec schemas PascalCase to the same ident).
                // 3. Target is a type alias whose resolution chain reaches
                //    the enclosing schema (catches cal-com's
                //    `ReassignBookingOutput20240813Data = Reassign...`
                //    pattern: the synthesized inline name aliases back to
                //    its parent).
                let enclosing_rust_name = self.to_rust_type_name(schema_name);
                let is_self_via_rust_name = target_rust_name == enclosing_rust_name;
                let is_alias_chain_self =
                    self.target_aliases_back_to(target, &enclosing_rust_name, analysis);
                if analysis.dependencies.recursive_schemas.contains(target)
                    || is_self_via_rust_name
                    || is_alias_chain_self
                {
                    quote! { Box<#target_type> }
                } else {
                    quote! { #target_type }
                }
            }
            SchemaType::Array { item_type } => {
                let inner_type = self.generate_array_item_type(item_type, analysis);
                quote! { Vec<#inner_type> }
            }
            _ => {
                // Fallback for complex types
                quote! { serde_json::Value }
            }
        }
    }

    fn generate_serde_field_attrs(
        &self,
        schema_name: &str,
        field_name: &str,
        field_ident: &syn::Ident,
        prop: &crate::analysis::PropertyInfo,
        is_required: bool,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> TokenStream {
        let mut attrs = Vec::new();

        // Generate rename attribute if field name differs from Rust identifier
        // Strip r# prefix for comparison since serde handles raw idents transparently
        let rust_field_name = field_ident.to_string();
        let comparison_name = rust_field_name
            .strip_prefix("r#")
            .unwrap_or(&rust_field_name);
        if comparison_name != field_name {
            attrs.push(quote! { rename = #field_name });
        }

        // Add skip_serializing_if for optional fields to avoid sending null values
        if !is_required || prop.nullable {
            attrs.push(quote! { skip_serializing_if = "Option::is_none" });
        }

        // Only add default attribute for required fields that have default values.
        // Skip #[serde(default)] for types that don't implement Default (discriminated
        // unions, union enums) — those fields should be Option<T> instead.
        if prop.default.is_some()
            && (is_required && !prop.nullable)
            && !self.type_lacks_default(&prop.schema_type, analysis)
        {
            attrs.push(quote! { default });
        }

        // Codec hint from TypeMapper (Q2): `format: byte` →
        // `with = "base64_serde"`, etc. Fields whose mapped type
        // carries no codec (e.g. chrono::DateTime<Utc> uses its
        // built-in serde) skip this attribute. Option fields need
        // the `::option` submodule of the codec — serde dispatches
        // on field type, and the base codec works on Vec<u8> /
        // chrono::Duration / etc., not their Option wrappers.
        if let crate::analysis::SchemaType::Primitive {
            serde_with: Some(codec),
            ..
        } = &prop.schema_type
        {
            let is_option_wrapped = self.property_is_option_wrapped(
                schema_name,
                field_name,
                prop,
                is_required,
                analysis,
            );
            let codec_path = if is_option_wrapped {
                format!("{codec}::option")
            } else {
                codec.clone()
            };
            attrs.push(quote! { with = #codec_path });
            // A `with` codec disables serde's implicit
            // missing-field → None handling for Option fields
            // (serde-rs/serde#2878); without `default` a request
            // that simply omits the field fails to deserialize.
            if is_option_wrapped {
                attrs.push(quote! { default });
            }
        }

        if attrs.is_empty() {
            TokenStream::new()
        } else {
            quote! { #[serde(#(#attrs),*)] }
        }
    }

    /// Check if a schema type resolves to a type that doesn't implement `Default`.
    /// Discriminated unions and union enums don't derive Default, so fields with
    /// these types can't use `#[serde(default)]`.
    fn type_lacks_default(
        &self,
        schema_type: &crate::analysis::SchemaType,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> bool {
        use crate::analysis::SchemaType;
        match schema_type {
            SchemaType::DiscriminatedUnion { .. } | SchemaType::Union { .. } => true,
            // Q2 typed scalars: chrono / url have no Default impl.
            // uuid::Uuid, bytes::Bytes, std::net::Ip*Addr all derive
            // Default, so they're safe to leave under #[serde(default)].
            SchemaType::Primitive { rust_type, .. } => matches!(
                rust_type.as_str(),
                "chrono::DateTime<chrono::Utc>"
                    | "chrono::NaiveDate"
                    | "chrono::NaiveTime"
                    | "chrono::Duration"
                    | "url::Url"
                    | "time::OffsetDateTime"
                    | "time::Date"
                    | "time::Time"
                    | "iso8601::Duration"
                    | "email_address::EmailAddress"
            ),
            SchemaType::Reference { target } => {
                if let Some(schema) = analysis.schemas.get(target) {
                    self.type_lacks_default(&schema.schema_type, analysis)
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn generate_specta_field_attrs(&self, field_name: &str) -> TokenStream {
        if !self.config.enable_specta {
            return TokenStream::new();
        }

        // Convert field name to camelCase for TypeScript
        let camel_case_name = self.to_camel_case(field_name);

        // Only add specta rename if it differs from the original field name
        if camel_case_name != field_name {
            quote! { #[cfg_attr(feature = "specta", specta(rename = #camel_case_name))] }
        } else {
            TokenStream::new()
        }
    }

    pub(crate) fn to_rust_enum_variant(&self, s: &str) -> String {
        // Preserve sign for numeric values so e.g. `-1` and `1` produce
        // distinct variants (`VariantNeg1` vs `Variant1`). Without this,
        // strict-namespace enums in github.json collide on `1`/`-1`.
        let neg_prefix =
            if s.starts_with('-') && s.chars().skip(1).all(|c| c.is_ascii_digit() || c == '.') {
                "Neg"
            } else {
                ""
            };

        // Convert string to valid Rust enum variant (PascalCase)
        let mut result = String::new();
        let mut next_upper = true;
        let mut prev_was_upper = false;

        for (i, c) in s.chars().enumerate() {
            match c {
                'a'..='z' => {
                    if next_upper {
                        result.push(c.to_ascii_uppercase());
                        next_upper = false;
                    } else {
                        result.push(c);
                    }
                    prev_was_upper = false;
                }
                'A'..='Z' => {
                    if next_upper || (!prev_was_upper && i > 0) {
                        // Start of word or transition from lowercase
                        result.push(c);
                        next_upper = false;
                    } else {
                        // Continue uppercase sequence, convert to lowercase
                        result.push(c.to_ascii_lowercase());
                    }
                    prev_was_upper = true;
                }
                '0'..='9' => {
                    result.push(c);
                    next_upper = false;
                    prev_was_upper = false;
                }
                '.' | '-' | '_' | ' ' | '@' | '#' | '$' | '/' | '\\' => {
                    // Word boundaries - next char should be uppercase
                    next_upper = true;
                    prev_was_upper = false;
                }
                _ => {
                    // Other special characters - treat as word boundary
                    next_upper = true;
                    prev_was_upper = false;
                }
            }
        }

        // Handle empty result
        if result.is_empty() {
            result = "Value".to_string();
        }

        // Ensure variant starts with a letter (not a number)
        if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            result = format!("Variant{neg_prefix}{result}");
        } else if !neg_prefix.is_empty() {
            // String happened to start with `-<digits>` but produced a
            // non-empty alphabetic prefix. Tag the negative anyway.
            result = format!("{neg_prefix}{result}");
        }

        // Handle special cases for enum variants
        match result.as_str() {
            "Null" => "NullValue".to_string(),
            "True" => "TrueValue".to_string(),
            "False" => "FalseValue".to_string(),
            "Type" => "Type_".to_string(),
            "Match" => "Match_".to_string(),
            "Fn" => "Fn_".to_string(),
            "Impl" => "Impl_".to_string(),
            "Trait" => "Trait_".to_string(),
            "Struct" => "Struct_".to_string(),
            "Enum" => "Enum_".to_string(),
            "Mod" => "Mod_".to_string(),
            "Use" => "Use_".to_string(),
            "Pub" => "Pub_".to_string(),
            "Const" => "Const_".to_string(),
            "Static" => "Static_".to_string(),
            "Let" => "Let_".to_string(),
            "Mut" => "Mut_".to_string(),
            "Ref" => "Ref_".to_string(),
            "Move" => "Move_".to_string(),
            "Return" => "Return_".to_string(),
            "If" => "If_".to_string(),
            "Else" => "Else_".to_string(),
            "While" => "While_".to_string(),
            "For" => "For_".to_string(),
            "Loop" => "Loop_".to_string(),
            "Break" => "Break_".to_string(),
            "Continue" => "Continue_".to_string(),
            "Self" => "Self_".to_string(),
            "Super" => "Super_".to_string(),
            "Crate" => "Crate_".to_string(),
            "Async" => "Async_".to_string(),
            "Await" => "Await_".to_string(),
            _ => result,
        }
    }

    #[allow(dead_code)]
    fn to_rust_identifier(&self, s: &str) -> String {
        // Convert string to valid Rust identifier
        let mut result = s
            .chars()
            .map(|c| match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' => c,
                '.' | '-' | '_' | ' ' | '@' | '#' | '$' | '/' | '\\' => '_',
                _ => '_',
            })
            .collect::<String>();

        // Remove leading/trailing underscores
        result = result.trim_matches('_').to_string();

        // Handle empty result
        if result.is_empty() {
            result = "value".to_string();
        }

        // Ensure identifier starts with a letter (not a number)
        if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            result = format!("variant_{result}");
        }

        // Handle special cases for enum values
        match result.as_str() {
            "null" => "null_value".to_string(),
            "true" => "true_value".to_string(),
            "false" => "false_value".to_string(),
            "type" => "type_".to_string(),
            "match" => "match_".to_string(),
            "fn" => "fn_".to_string(),
            "impl" => "impl_".to_string(),
            "trait" => "trait_".to_string(),
            "struct" => "struct_".to_string(),
            "enum" => "enum_".to_string(),
            "mod" => "mod_".to_string(),
            "use" => "use_".to_string(),
            "pub" => "pub_".to_string(),
            "const" => "const_".to_string(),
            "static" => "static_".to_string(),
            "let" => "let_".to_string(),
            "mut" => "mut_".to_string(),
            "ref" => "ref_".to_string(),
            "move" => "move_".to_string(),
            "return" => "return_".to_string(),
            "if" => "if_".to_string(),
            "else" => "else_".to_string(),
            "while" => "while_".to_string(),
            "for" => "for_".to_string(),
            "loop" => "loop_".to_string(),
            "break" => "break_".to_string(),
            "continue" => "continue_".to_string(),
            "self" => "self_".to_string(),
            "super" => "super_".to_string(),
            "crate" => "crate_".to_string(),
            "async" => "async_".to_string(),
            "await" => "await_".to_string(),
            // Reserved keywords for edition 2018+
            "override" => "override_".to_string(),
            "box" => "box_".to_string(),
            "dyn" => "dyn_".to_string(),
            "where" => "where_".to_string(),
            "in" => "in_".to_string(),
            // Reserved for future use
            "abstract" => "abstract_".to_string(),
            "become" => "become_".to_string(),
            "do" => "do_".to_string(),
            "final" => "final_".to_string(),
            "macro" => "macro_".to_string(),
            "priv" => "priv_".to_string(),
            "try" => "try_".to_string(),
            "typeof" => "typeof_".to_string(),
            "unsized" => "unsized_".to_string(),
            "virtual" => "virtual_".to_string(),
            "yield" => "yield_".to_string(),
            _ => result,
        }
    }

    /// Q2.4: render a `/// Constraint: …` doc comment for a field
    /// when its OpenAPI schema declares any constraint annotations.
    /// No-op when constraints are empty or `mode = "off"`.
    ///
    /// **Doc-comment only** — by deliberate design we never emit
    /// `#[validate(...)]` attributes. Constraints belong to the wire
    /// contract; the server is the source of truth.
    fn generate_constraint_doc(
        &self,
        constraints: &crate::analysis::PropertyConstraints,
    ) -> TokenStream {
        use crate::type_mapping::ConstraintMode;

        if constraints.is_empty() {
            return TokenStream::new();
        }
        match self.config.types.constraint_mode() {
            ConstraintMode::Off => TokenStream::new(),
            ConstraintMode::Doc => {
                let formatted = format_constraints_doc(constraints);
                quote! { #[doc = #formatted] }
            }
        }
    }

    fn sanitize_doc_comment(&self, desc: &str) -> String {
        // Sanitize description to prevent doctest failures
        let mut result = desc.to_string();

        // Look for potential code examples that might be interpreted as doctests
        // Common patterns that cause issues:
        // - Lines that look like standalone expressions
        // - JSON-like content
        // - Template strings with {}

        // If the description contains what looks like code, wrap it in a text block
        if result.contains('\n')
            && (result.contains('{')
                || result.contains("```")
                || result.contains("Human:")
                || result.contains("Assistant:")
                || result
                    .lines()
                    .any(|line| line.trim().starts_with('"') && line.trim().ends_with('"')))
        {
            // If it already has code blocks, add ignore annotation
            if result.contains("```") {
                result = result.replace("```", "```ignore");
            } else {
                // Wrap the entire description in an ignored code block if it looks like code
                if result.lines().any(|line| {
                    let trimmed = line.trim();
                    trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() > 2
                }) {
                    result = format!("```ignore\n{result}\n```");
                }
            }
        }

        result
    }

    pub(crate) fn to_rust_type_name(&self, s: &str) -> String {
        // Convert string to valid Rust type name (PascalCase)
        let mut result = String::new();
        let mut next_upper = true;
        let mut prev_was_lower = false;

        for c in s.chars() {
            match c {
                'a'..='z' => {
                    if next_upper {
                        result.push(c.to_ascii_uppercase());
                        next_upper = false;
                    } else {
                        result.push(c);
                    }
                    prev_was_lower = true;
                }
                'A'..='Z' => {
                    result.push(c);
                    next_upper = false;
                    prev_was_lower = false;
                }
                '0'..='9' => {
                    // If previous was lowercase letter and this is start of a number sequence,
                    // make it uppercase to improve readability (e.g., Tool20241022 instead of Tool20241022)
                    if prev_was_lower && !result.chars().last().unwrap_or(' ').is_ascii_digit() {
                        // This is fine as-is, the number follows naturally
                    }
                    result.push(c);
                    next_upper = false;
                    prev_was_lower = false;
                }
                '_' | '-' | '.' | ' ' => {
                    // Skip underscore/separator and make next char uppercase
                    next_upper = true;
                    prev_was_lower = false;
                }
                _ => {
                    // Other special characters - treat as word boundary
                    next_upper = true;
                    prev_was_lower = false;
                }
            }
        }

        // Handle empty result
        if result.is_empty() {
            result = "Type".to_string();
        }

        // Ensure type name starts with a letter (not a number)
        if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            result = format!("Type{result}");
        }

        // Avoid masking ubiquitous std types and traits. cloudflare has a
        // schema literally named `Result`, gcore has `Default`; emitting
        // `pub enum Result { ... }` shadows std::result::Result and breaks
        // every method's `-> Result<T, ApiOpError<...>>`. Same for impls
        // like `impl Default for HttpClient { ... }` when `Default` resolves
        // to the local type alias.
        if matches!(
            result.as_str(),
            "Result"
                | "Option"
                | "Box"
                | "Vec"
                | "String"
                | "Some"
                | "None"
                | "Ok"
                | "Err"
                | "Default"
                | "Clone"
                | "Debug"
                | "Send"
                | "Sync"
                | "Sized"
                | "Iterator"
                | "From"
                | "Into"
                | "TryFrom"
                | "TryInto"
                | "AsRef"
                | "AsMut"
        ) {
            result.push_str("Type");
        }

        result
    }

    fn to_rust_field_name(&self, s: &str) -> String {
        // Track sign / leading-non-alpha so e.g. `+1` and `-1` produce
        // distinct field names instead of both collapsing to `field_1`
        // (observed in github.json's reactions schemas).
        let leading_marker = match s.chars().next() {
            Some('-') if s.len() > 1 => "neg_",
            Some('+') if s.len() > 1 => "pos_",
            _ => "",
        };

        // Convert field name to snake_case properly
        let mut result = String::new();
        let mut prev_was_upper = false;
        let mut prev_was_underscore = false;

        for (i, c) in s.chars().enumerate() {
            match c {
                'A'..='Z' => {
                    // Add underscore before uppercase if previous was lowercase
                    if i > 0 && !prev_was_upper && !prev_was_underscore {
                        result.push('_');
                    }
                    result.push(c.to_ascii_lowercase());
                    prev_was_upper = true;
                    prev_was_underscore = false;
                }
                'a'..='z' | '0'..='9' => {
                    result.push(c);
                    prev_was_upper = false;
                    prev_was_underscore = false;
                }
                '-' | '.' | '_' | '@' | '#' | '$' | ' ' => {
                    if !prev_was_underscore && !result.is_empty() {
                        result.push('_');
                        prev_was_underscore = true;
                    }
                    prev_was_upper = false;
                }
                _ => {
                    // For other special characters, convert to underscore
                    if !prev_was_underscore && !result.is_empty() {
                        result.push('_');
                    }
                    prev_was_upper = false;
                    prev_was_underscore = true;
                }
            }
        }

        // Clean up result
        let mut result = result.trim_matches('_').to_string();
        if result.is_empty() {
            return "field".to_string();
        }

        // Ensure field name starts with a letter or underscore (not a number)
        if result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            result = format!("field_{leading_marker}{result}");
        } else if !leading_marker.is_empty() {
            result = format!("{leading_marker}{result}");
        }

        // `self`, `super`, `crate`, `Self` are NOT permitted as raw identifiers
        // (they trigger an `r#self cannot be a raw identifier` panic in
        // proc_macro2). Suffix them instead.
        if matches!(result.as_str(), "self" | "super" | "crate" | "Self") {
            return format!("{result}_field");
        }
        // Handle reserved keywords using raw identifiers (r#keyword)
        if Self::is_rust_keyword(&result) {
            format!("r#{result}")
        } else {
            result
        }
    }

    /// Check if a string is a Rust keyword that needs raw identifier treatment
    pub fn is_rust_keyword(s: &str) -> bool {
        matches!(
            s,
            "type"
                | "match"
                | "fn"
                | "struct"
                | "enum"
                | "impl"
                | "trait"
                | "mod"
                | "use"
                | "pub"
                | "const"
                | "static"
                | "let"
                | "mut"
                | "ref"
                | "move"
                | "return"
                | "if"
                | "else"
                | "while"
                | "for"
                | "loop"
                | "break"
                | "continue"
                | "self"
                | "super"
                | "crate"
                | "async"
                | "await"
                | "override"
                | "box"
                | "dyn"
                | "where"
                | "in"
                | "abstract"
                | "become"
                | "do"
                | "final"
                | "macro"
                | "priv"
                | "try"
                | "typeof"
                | "unsized"
                | "virtual"
                | "yield"
                // Rust 2024 edition reservations.
                | "gen"
        )
    }

    /// Create a proc_macro2::Ident from a field name, handling r# raw identifiers
    pub fn to_field_ident(name: &str) -> proc_macro2::Ident {
        if let Some(raw) = name.strip_prefix("r#") {
            proc_macro2::Ident::new_raw(raw, proc_macro2::Span::call_site())
        } else {
            proc_macro2::Ident::new(name, proc_macro2::Span::call_site())
        }
    }

    fn to_camel_case(&self, s: &str) -> String {
        // Convert snake_case or other formats to camelCase
        let mut result = String::new();
        let mut capitalize_next = false;

        for (i, c) in s.chars().enumerate() {
            match c {
                '_' | '-' | '.' | ' ' => {
                    // Word boundary - capitalize next letter
                    capitalize_next = true;
                }
                'A'..='Z' => {
                    if i == 0 {
                        // First character should be lowercase in camelCase
                        result.push(c.to_ascii_lowercase());
                    } else if capitalize_next {
                        result.push(c);
                        capitalize_next = false;
                    } else {
                        result.push(c.to_ascii_lowercase());
                    }
                }
                'a'..='z' | '0'..='9' => {
                    if capitalize_next {
                        result.push(c.to_ascii_uppercase());
                        capitalize_next = false;
                    } else {
                        result.push(c);
                    }
                }
                _ => {
                    // Other characters - treat as word boundary
                    capitalize_next = true;
                }
            }
        }

        if result.is_empty() {
            return "field".to_string();
        }

        result
    }

    fn generate_composition_struct(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        schemas: &[crate::analysis::SchemaRef],
    ) -> Result<TokenStream> {
        let struct_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        // For composition, we can either:
        // 1. Flatten all referenced schemas into one struct (if they're all objects)
        // 2. Use serde(flatten) to compose them at runtime
        // For now, let's use approach 2 with serde(flatten)

        let fields = schemas.iter().enumerate().map(|(i, schema_ref)| {
            let field_name = format_ident!("part_{}", i);
            let field_type = format_ident!("{}", self.to_rust_type_name(&schema_ref.target));

            quote! {
                #[serde(flatten)]
                pub #field_name: #field_type,
            }
        });

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // Generate derives with optional Specta support
        let derives = if self.config.enable_specta {
            quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
            }
        } else {
            quote! {
                #[derive(Debug, Clone, Deserialize, Serialize)]
            }
        };

        Ok(quote! {
            #doc_comment
            #derives
            pub struct #struct_name {
                #(#fields)*
            }
        })
    }

    #[allow(dead_code)]
    fn find_missing_types(&self, analysis: &SchemaAnalysis) -> std::collections::HashSet<String> {
        let mut missing = std::collections::HashSet::new();
        let defined_types: std::collections::HashSet<String> =
            analysis.schemas.keys().cloned().collect();

        // Check all references in union variants
        for schema in analysis.schemas.values() {
            match &schema.schema_type {
                crate::analysis::SchemaType::Union { variants } => {
                    for variant in variants {
                        if !defined_types.contains(&variant.target) {
                            missing.insert(variant.target.clone());
                        }
                    }
                }
                crate::analysis::SchemaType::DiscriminatedUnion { variants, .. } => {
                    for variant in variants {
                        if !defined_types.contains(&variant.type_name) {
                            missing.insert(variant.type_name.clone());
                        }
                    }
                }
                crate::analysis::SchemaType::Object { properties, .. } => {
                    // Sort properties for deterministic iteration
                    let mut sorted_props: Vec<_> = properties.iter().collect();
                    sorted_props.sort_by_key(|(name, _)| name.as_str());
                    for (_, prop) in sorted_props {
                        if let crate::analysis::SchemaType::Reference { target } = &prop.schema_type
                        {
                            if !defined_types.contains(target) {
                                missing.insert(target.clone());
                            }
                        }
                    }
                }
                crate::analysis::SchemaType::Reference { target }
                    if !defined_types.contains(target) =>
                {
                    missing.insert(target.clone());
                }
                _ => {}
            }
        }

        missing
    }

    #[allow(clippy::only_used_in_recursion)]
    fn generate_array_item_type(
        &self,
        item_type: &crate::analysis::SchemaType,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> TokenStream {
        use crate::analysis::SchemaType;

        match item_type {
            SchemaType::Primitive { rust_type, .. } => {
                // The string here may be anything from `i64` / `String` to
                // `serde_json::Value` to `Vec<serde_json::Value>` to
                // `BTreeMap<String, T>`. Parse it as a syn::Type so we get
                // the right tokens regardless of generics.
                if let Ok(parsed) = syn::parse_str::<syn::Type>(rust_type) {
                    quote! { #parsed }
                } else if rust_type.contains("::") {
                    let parts: Vec<_> = rust_type
                        .split("::")
                        .map(|p| format_ident!("{}", p))
                        .collect();
                    quote! { #(#parts)::* }
                } else {
                    let type_ident = format_ident!("{}", rust_type);
                    quote! { #type_ident }
                }
            }
            SchemaType::Reference { target } => {
                let target_type = format_ident!("{}", self.to_rust_type_name(target));
                // Wrap recursive references in Box<T> for heap allocation in arrays
                if analysis.dependencies.recursive_schemas.contains(target) {
                    quote! { Box<#target_type> }
                } else {
                    quote! { #target_type }
                }
            }
            SchemaType::Array { item_type } => {
                // Nested arrays
                let inner_type = self.generate_array_item_type(item_type, analysis);
                quote! { Vec<#inner_type> }
            }
            _ => {
                // Fallback for complex types
                quote! { serde_json::Value }
            }
        }
    }

    /// Convert a type name to a variant name (e.g., OutputMessage -> OutputMessage, FileSearchToolCall -> FileSearchToolCall)
    fn type_name_to_variant_name(&self, type_name: &str) -> String {
        // Handle primitive types specially
        match type_name {
            "bool" => return "Boolean".to_string(),
            "i8" | "i16" | "i32" | "i64" | "i128" => return "Integer".to_string(),
            "u8" | "u16" | "u32" | "u64" | "u128" => return "UnsignedInteger".to_string(),
            "f32" | "f64" => return "Number".to_string(),
            "String" => return "String".to_string(),
            "serde_json::Value" => return "Value".to_string(),
            // Q2 typed-scalar paths. Without these the fallback PascalCase
            // pass over `bytes::Bytes` produces `BytesBytes(BytesBytes)`,
            // which then can't compile because no `BytesBytes` type exists.
            "bytes::Bytes" => return "Binary".to_string(),
            "chrono::DateTime<chrono::Utc>" => return "DateTime".to_string(),
            "chrono::NaiveDate" => return "Date".to_string(),
            "chrono::NaiveTime" => return "Time".to_string(),
            "uuid::Uuid" => return "Uuid".to_string(),
            "url::Url" => return "Url".to_string(),
            "std::net::Ipv4Addr" => return "Ipv4".to_string(),
            "std::net::Ipv6Addr" => return "Ipv6".to_string(),
            _ => {}
        }

        // Handle Vec types
        if type_name.starts_with("Vec<") && type_name.ends_with(">") {
            let inner = &type_name[4..type_name.len() - 1];
            // Handle nested Vec types specially
            if inner.starts_with("Vec<") && inner.ends_with(">") {
                let inner_inner = &inner[4..inner.len() - 1];
                return format!("{}ArrayArray", self.type_name_to_variant_name(inner_inner));
            }
            return format!("{}Array", self.type_name_to_variant_name(inner));
        }

        // For untagged unions, we want to use the type name itself as the variant name
        // since it's already meaningful. This gives us OutputMessage instead of Variant0,
        // FileSearchToolCall instead of Variant1, etc.

        // Remove common suffixes that might make variant names redundant
        let clean_name = type_name
            .trim_end_matches("Type")
            .trim_end_matches("Schema")
            .trim_end_matches("Item");

        // Always convert to proper PascalCase to ensure no underscores in enum variants
        self.to_rust_type_name(clean_name)
    }

    /// Ensure unique variant name for generator (similar to analyzer but for generator context)
    fn ensure_unique_variant_name_generator(
        &self,
        base_name: String,
        used_names: &mut std::collections::HashSet<String>,
        fallback_index: usize,
    ) -> String {
        if used_names.insert(base_name.clone()) {
            return base_name;
        }

        // Try with numbers
        for i in 2..100 {
            let numbered_name = format!("{base_name}{i}");
            if used_names.insert(numbered_name.clone()) {
                return numbered_name;
            }
        }

        // Fallback to Variant{index} if all else fails
        let fallback = format!("Variant{fallback_index}");
        used_names.insert(fallback.clone());
        fallback
    }

    /// Find the request type for a given operation ID using the analyzed operation info
    fn find_request_type_for_operation(
        &self,
        operation_id: &str,
        analysis: &SchemaAnalysis,
    ) -> Option<String> {
        // Use the operation analysis to get the actual request body schema
        analysis.operations.get(operation_id).and_then(|op| {
            op.request_body
                .as_ref()
                .and_then(|rb| rb.schema_name().map(|s| s.to_string()))
        })
    }

    /// Resolve the correct streaming event type based on EventFlow pattern
    fn resolve_streaming_event_type(
        &self,
        endpoint: &crate::streaming::StreamingEndpoint,
        analysis: &SchemaAnalysis,
    ) -> Result<String> {
        match &endpoint.event_flow {
            crate::streaming::EventFlow::Simple => {
                // For simple streaming, use the response type directly
                // Validate that the specified type exists in the schema
                if analysis.schemas.contains_key(&endpoint.event_union_type) {
                    Ok(endpoint.event_union_type.to_string())
                } else {
                    Err(crate::error::GeneratorError::ValidationError(format!(
                        "Streaming response type '{}' not found in schema for simple streaming endpoint '{}'",
                        endpoint.event_union_type, endpoint.operation_id
                    )))
                }
            }
            crate::streaming::EventFlow::StartDeltaStop { .. } => {
                // For complex event-based streaming, ensure we have a proper union type
                // For now, use the specified event_union_type but add validation
                if analysis.schemas.contains_key(&endpoint.event_union_type) {
                    Ok(endpoint.event_union_type.to_string())
                } else {
                    Err(crate::error::GeneratorError::ValidationError(format!(
                        "Event union type '{}' not found in schema for complex streaming endpoint '{}'",
                        endpoint.event_union_type, endpoint.operation_id
                    )))
                }
            }
        }
    }

    /// Generate streaming error types
    fn generate_streaming_error_types(&self) -> Result<TokenStream> {
        Ok(quote! {
            /// Error type for streaming operations
            #[derive(Debug, thiserror::Error)]
            pub enum StreamingError {
                #[error("Connection error: {0}")]
                Connection(String),
                #[error("HTTP error: {status}")]
                Http { status: u16 },
                #[error("SSE parsing error: {0}")]
                Parsing(String),
                #[error("Authentication error: {0}")]
                Authentication(String),
                #[error("Rate limit error: {0}")]
                RateLimit(String),
                #[error("API error: {0}")]
                Api(String),
                #[error("Timeout error: {0}")]
                Timeout(String),
                #[error("JSON serialization/deserialization error: {0}")]
                Json(#[from] serde_json::Error),
                #[error("Request error: {0}")]
                Request(reqwest::Error),
            }

            impl From<reqwest::header::InvalidHeaderValue> for StreamingError {
                fn from(err: reqwest::header::InvalidHeaderValue) -> Self {
                    StreamingError::Api(format!("Invalid header value: {}", err))
                }
            }

            impl From<reqwest::Error> for StreamingError {
                fn from(err: reqwest::Error) -> Self {
                    if err.is_timeout() {
                        StreamingError::Timeout(err.to_string())
                    } else if err.is_status() {
                        if let Some(status) = err.status() {
                            StreamingError::Http { status: status.as_u16() }
                        } else {
                            StreamingError::Connection(err.to_string())
                        }
                    } else {
                        StreamingError::Request(err)
                    }
                }
            }
        })
    }

    /// Generate trait for a streaming endpoint
    fn generate_endpoint_trait(
        &self,
        endpoint: &crate::streaming::StreamingEndpoint,
        analysis: &SchemaAnalysis,
    ) -> Result<TokenStream> {
        use crate::streaming::HttpMethod;

        let trait_name = format_ident!(
            "{}StreamingClient",
            self.to_rust_type_name(&endpoint.operation_id)
        );
        let method_name =
            format_ident!("stream_{}", self.to_rust_field_name(&endpoint.operation_id));
        let event_type =
            format_ident!("{}", self.resolve_streaming_event_type(endpoint, analysis)?);

        // Generate method signature based on HTTP method
        let method_signature = match endpoint.http_method {
            HttpMethod::Get => {
                // Generate parameters from query_parameters
                let mut param_defs = Vec::new();
                for qp in &endpoint.query_parameters {
                    let param_name = format_ident!("{}", self.to_rust_field_name(&qp.name));
                    if qp.required {
                        param_defs.push(quote! { #param_name: &str });
                    } else {
                        param_defs.push(quote! { #param_name: Option<&str> });
                    }
                }
                quote! {
                    async fn #method_name(
                        &self,
                        #(#param_defs),*
                    ) -> Result<Pin<Box<dyn Stream<Item = Result<#event_type, Self::Error>> + Send>>, Self::Error>;
                }
            }
            HttpMethod::Post => {
                // Find the request type for this operation
                let request_type = self
                    .find_request_type_for_operation(&endpoint.operation_id, analysis)
                    .unwrap_or_else(|| "serde_json::Value".to_string());
                let request_type_ident = if request_type.contains("::") {
                    let parts: Vec<&str> = request_type.split("::").collect();
                    let path_parts: Vec<_> = parts.iter().map(|p| format_ident!("{}", p)).collect();
                    quote! { #(#path_parts)::* }
                } else {
                    let ident = format_ident!("{}", request_type);
                    quote! { #ident }
                };
                quote! {
                    async fn #method_name(
                        &self,
                        request: #request_type_ident,
                    ) -> Result<Pin<Box<dyn Stream<Item = Result<#event_type, Self::Error>> + Send>>, Self::Error>;
                }
            }
        };

        Ok(quote! {
            /// Streaming client trait for this endpoint
            #[async_trait]
            pub trait #trait_name {
                type Error: std::error::Error + Send + Sync + 'static;

                /// Stream events from the API
                #method_signature
            }
        })
    }

    /// Generate streaming client implementation
    fn generate_streaming_client_impl(
        &self,
        streaming_config: &crate::streaming::StreamingConfig,
        analysis: &SchemaAnalysis,
    ) -> Result<TokenStream> {
        let client_name = format_ident!(
            "{}Client",
            self.to_rust_type_name(&streaming_config.client_module_name)
        );

        // Generate struct fields
        // Always include custom_headers for flexibility (like HttpClient does)
        let mut struct_fields = vec![
            quote! { base_url: String },
            quote! { api_key: Option<String> },
            quote! { http_client: reqwest::Client },
            quote! { custom_headers: std::collections::BTreeMap<String, String> },
        ];

        let has_optional_headers = !streaming_config
            .endpoints
            .iter()
            .all(|e| e.optional_headers.is_empty());

        if has_optional_headers {
            struct_fields
                .push(quote! { optional_headers: std::collections::BTreeMap<String, String> });
        }

        // Generate constructor
        // Use configured base URL as default, or fallback to generic example
        let default_base_url = if let Some(ref streaming_config) = self.config.streaming_config {
            streaming_config
                .endpoints
                .first()
                .and_then(|e| e.base_url.as_deref())
                .unwrap_or("https://api.example.com")
        } else {
            "https://api.example.com"
        };

        // Build constructor fields based on what the struct has
        let constructor_fields = if has_optional_headers {
            quote! {
                base_url: #default_base_url.to_string(),
                api_key: None,
                http_client: reqwest::Client::new(),
                custom_headers: std::collections::BTreeMap::new(),
                optional_headers: std::collections::BTreeMap::new(),
            }
        } else {
            quote! {
                base_url: #default_base_url.to_string(),
                api_key: None,
                http_client: reqwest::Client::new(),
                custom_headers: std::collections::BTreeMap::new(),
            }
        };

        // Optional headers method only if the struct has the field
        let optional_headers_method = if has_optional_headers {
            quote! {
                /// Set optional headers for all requests
                pub fn set_optional_headers(&mut self, headers: std::collections::BTreeMap<String, String>) {
                    self.optional_headers = headers;
                }
            }
        } else {
            TokenStream::new()
        };

        let constructor = quote! {
            impl #client_name {
                /// Create a new streaming client
                pub fn new() -> Self {
                    Self {
                        #constructor_fields
                    }
                }

                /// Set the base URL for API requests
                pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
                    self.base_url = base_url.into();
                    self
                }

                /// Set the API key for authentication
                pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
                    self.api_key = Some(api_key.into());
                    self
                }

                /// Add a custom header to all requests
                pub fn with_header(
                    mut self,
                    name: impl Into<String>,
                    value: impl Into<String>,
                ) -> Self {
                    self.custom_headers.insert(name.into(), value.into());
                    self
                }

                /// Set the HTTP client
                pub fn with_http_client(mut self, client: reqwest::Client) -> Self {
                    self.http_client = client;
                    self
                }

                #optional_headers_method
            }
        };

        // Generate trait implementations for each endpoint
        let mut trait_impls = Vec::new();
        for endpoint in &streaming_config.endpoints {
            let trait_impl = self.generate_endpoint_trait_impl(endpoint, &client_name, analysis)?;
            trait_impls.push(trait_impl);
        }

        // Add Default implementation
        let default_impl = quote! {
            impl Default for #client_name {
                fn default() -> Self {
                    Self::new()
                }
            }
        };

        Ok(quote! {
            /// Streaming client implementation
            #[derive(Debug, Clone)]
            pub struct #client_name {
                #(#struct_fields,)*
            }

            #constructor

            #default_impl

            #(#trait_impls)*
        })
    }

    /// Generate trait implementation for a specific endpoint
    fn generate_endpoint_trait_impl(
        &self,
        endpoint: &crate::streaming::StreamingEndpoint,
        client_name: &proc_macro2::Ident,
        analysis: &SchemaAnalysis,
    ) -> Result<TokenStream> {
        use crate::streaming::HttpMethod;

        let trait_name = format_ident!(
            "{}StreamingClient",
            self.to_rust_type_name(&endpoint.operation_id)
        );
        let method_name =
            format_ident!("stream_{}", self.to_rust_field_name(&endpoint.operation_id));
        let event_type =
            format_ident!("{}", self.resolve_streaming_event_type(endpoint, analysis)?);

        // Generate required headers
        let mut header_setup = Vec::new();
        for (name, value) in &endpoint.required_headers {
            header_setup.push(quote! {
                headers.insert(#name, HeaderValue::from_static(#value));
            });
        }

        // Add authentication header
        // If auth_header is configured, use that; otherwise default to Bearer auth on Authorization header
        if let Some(auth_header) = &endpoint.auth_header {
            match auth_header {
                crate::streaming::AuthHeader::Bearer(header_name) => {
                    header_setup.push(quote! {
                        if let Some(ref api_key) = self.api_key {
                            headers.insert(#header_name, HeaderValue::from_str(&format!("Bearer {}", api_key))?);
                        }
                    });
                }
                crate::streaming::AuthHeader::ApiKey(header_name) => {
                    header_setup.push(quote! {
                        if let Some(ref api_key) = self.api_key {
                            headers.insert(#header_name, HeaderValue::from_str(api_key)?);
                        }
                    });
                }
            }
        } else {
            // Default: use api_key as Bearer token on Authorization header
            header_setup.push(quote! {
                if let Some(ref api_key) = self.api_key {
                    headers.insert("Authorization", HeaderValue::from_str(&format!("Bearer {}", api_key))?);
                }
            });
        }

        // Always add custom_headers (like HttpClient does)
        header_setup.push(quote! {
            for (name, value) in &self.custom_headers {
                if let (Ok(header_name), Ok(header_value)) = (reqwest::header::HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value)) {
                    headers.insert(header_name, header_value);
                }
            }
        });

        // Add optional headers (for endpoint-specific optional headers)
        if !endpoint.optional_headers.is_empty() {
            header_setup.push(quote! {
                for (key, value) in &self.optional_headers {
                    if let (Ok(header_name), Ok(header_value)) = (reqwest::header::HeaderName::from_bytes(key.as_bytes()), HeaderValue::from_str(value)) {
                        headers.insert(header_name, header_value);
                    }
                }
            });
        }

        // Generate different code for GET vs POST
        match endpoint.http_method {
            HttpMethod::Get => self.generate_get_streaming_impl(
                endpoint,
                client_name,
                &trait_name,
                &method_name,
                &event_type,
                &header_setup,
            ),
            HttpMethod::Post => self.generate_post_streaming_impl(
                endpoint,
                client_name,
                &trait_name,
                &method_name,
                &event_type,
                &header_setup,
                analysis,
            ),
        }
    }

    /// Generate streaming implementation for GET endpoints
    fn generate_get_streaming_impl(
        &self,
        endpoint: &crate::streaming::StreamingEndpoint,
        client_name: &proc_macro2::Ident,
        trait_name: &proc_macro2::Ident,
        method_name: &proc_macro2::Ident,
        event_type: &proc_macro2::Ident,
        header_setup: &[TokenStream],
    ) -> Result<TokenStream> {
        let path = &endpoint.path;

        // Generate method parameters from query_parameters
        let mut param_defs = Vec::new();
        let mut query_params = Vec::new();

        for qp in &endpoint.query_parameters {
            let param_name = format_ident!("{}", self.to_rust_field_name(&qp.name));
            let param_name_str = &qp.name;

            if qp.required {
                param_defs.push(quote! { #param_name: &str });
                query_params.push(quote! {
                    url.query_pairs_mut().append_pair(#param_name_str, #param_name);
                });
            } else {
                param_defs.push(quote! { #param_name: Option<&str> });
                query_params.push(quote! {
                    if let Some(v) = #param_name {
                        url.query_pairs_mut().append_pair(#param_name_str, v);
                    }
                });
            }
        }

        // Generate URL construction for GET
        let url_construction = quote! {
            let base_url = url::Url::parse(&self.base_url)
                .map_err(|e| StreamingError::Connection(format!("Invalid base URL: {}", e)))?;
            let path_to_join = #path.trim_start_matches('/');
            let mut url = base_url.join(path_to_join)
                .map_err(|e| StreamingError::Connection(format!("URL join error: {}", e)))?;
            #(#query_params)*
        };

        let instrument_skip = quote! { #[instrument(skip(self), name = "streaming_get_request")] };

        Ok(quote! {
            #[async_trait]
            impl #trait_name for #client_name {
                type Error = StreamingError;

                #instrument_skip
                async fn #method_name(
                    &self,
                    #(#param_defs),*
                ) -> Result<Pin<Box<dyn Stream<Item = Result<#event_type, Self::Error>> + Send>>, Self::Error> {
                    debug!("Starting streaming GET request");

                    let mut headers = HeaderMap::new();
                    #(#header_setup)*

                    #url_construction
                    let url_str = url.to_string();
                    debug!("Making streaming GET request to: {}", url_str);

                    let request_builder = self.http_client
                        .get(url_str)
                        .headers(headers);

                    debug!("Creating SSE stream from request");
                    let stream = parse_sse_stream::<#event_type>(request_builder).await?;
                    info!("SSE stream created successfully");
                    Ok(Box::pin(stream))
                }
            }
        })
    }

    /// Generate streaming implementation for POST endpoints
    #[allow(clippy::too_many_arguments)]
    fn generate_post_streaming_impl(
        &self,
        endpoint: &crate::streaming::StreamingEndpoint,
        client_name: &proc_macro2::Ident,
        trait_name: &proc_macro2::Ident,
        method_name: &proc_macro2::Ident,
        event_type: &proc_macro2::Ident,
        header_setup: &[TokenStream],
        analysis: &SchemaAnalysis,
    ) -> Result<TokenStream> {
        let path = &endpoint.path;

        // Find the request type for this operation
        let request_type = self
            .find_request_type_for_operation(&endpoint.operation_id, analysis)
            .unwrap_or_else(|| "serde_json::Value".to_string());
        let request_type_ident = if request_type.contains("::") {
            let parts: Vec<&str> = request_type.split("::").collect();
            let path_parts: Vec<_> = parts.iter().map(|p| format_ident!("{}", p)).collect();
            quote! { #(#path_parts)::* }
        } else {
            let ident = format_ident!("{}", request_type);
            quote! { #ident }
        };

        // Generate URL construction for POST
        let url_construction = quote! {
            let base_url = url::Url::parse(&self.base_url)
                .map_err(|e| StreamingError::Connection(format!("Invalid base URL: {}", e)))?;
            let path_to_join = #path.trim_start_matches('/');
            let url = base_url.join(path_to_join)
                .map_err(|e| StreamingError::Connection(format!("URL join error: {}", e)))?
                .to_string();
        };

        // Generate stream parameter setup (only for POST with stream_parameter)
        let stream_param = &endpoint.stream_parameter;
        let stream_setup = if stream_param.is_empty() {
            quote! {
                let streaming_request = request;
            }
        } else {
            quote! {
                // Ensure streaming is enabled
                let mut streaming_request = request;
                if let Ok(mut request_value) = serde_json::to_value(&streaming_request) {
                    if let Some(obj) = request_value.as_object_mut() {
                        obj.insert(#stream_param.to_string(), serde_json::Value::Bool(true));
                    }
                    streaming_request = serde_json::from_value(request_value)?;
                }
            }
        };

        Ok(quote! {
            #[async_trait]
            impl #trait_name for #client_name {
                type Error = StreamingError;

                #[instrument(skip(self, request), name = "streaming_post_request")]
                async fn #method_name(
                    &self,
                    request: #request_type_ident,
                ) -> Result<Pin<Box<dyn Stream<Item = Result<#event_type, Self::Error>> + Send>>, Self::Error> {
                    debug!("Starting streaming POST request");

                    #stream_setup

                    let mut headers = HeaderMap::new();
                    #(#header_setup)*

                    #url_construction
                    debug!("Making streaming POST request to: {}", url);

                    let request_builder = self.http_client
                        .post(&url)
                        .headers(headers)
                        .json(&streaming_request);

                    debug!("Creating SSE stream from request");
                    let stream = parse_sse_stream::<#event_type>(request_builder).await?;
                    info!("SSE stream created successfully");
                    Ok(Box::pin(stream))
                }
            }
        })
    }

    /// Generate SSE parsing utilities using reqwest-eventsource
    fn generate_sse_parser_utilities(
        &self,
        _streaming_config: &crate::streaming::StreamingConfig,
    ) -> Result<TokenStream> {
        Ok(quote! {
            /// Parse SSE stream from HTTP request using reqwest-eventsource
            pub async fn parse_sse_stream<T>(
                request_builder: reqwest::RequestBuilder
            ) -> Result<impl Stream<Item = Result<T, StreamingError>>, StreamingError>
            where
                T: serde::de::DeserializeOwned + Send + 'static,
            {
                let mut event_source = reqwest_eventsource::EventSource::new(request_builder).map_err(|e| {
                    StreamingError::Connection(format!("Failed to create event source: {}", e))
                })?;

                let stream = event_source.filter_map(|event_result| async move {
                    match event_result {
                        Ok(reqwest_eventsource::Event::Open) => {
                            debug!("SSE connection opened");
                            None
                        }
                        Ok(reqwest_eventsource::Event::Message(message)) => {
                            // Check if this is a ping event by SSE event type
                            if message.event == "ping" {
                                debug!("Received SSE ping event, skipping");
                                return None;
                            }

                            // Special handling for empty data
                            if message.data.trim().is_empty() {
                                debug!("Empty SSE data, skipping");
                                return None;
                            }

                            // Check if this is a ping event in the JSON data
                            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&message.data) {
                                if let Some(event_type) = json_value.get("event").and_then(|v| v.as_str()) {
                                    if event_type == "ping" {
                                        debug!("Received ping event in JSON data, skipping");
                                        return None;
                                    }
                                }

                                // Try to parse the full event normally
                                match serde_json::from_value::<T>(json_value) {
                                    Ok(parsed_event) => {
                                        Some(Ok(parsed_event))
                                    }
                                    Err(e) => {
                                        if message.data.contains("ping") || message.event.contains("ping") {
                                            debug!("Ignoring ping-related event: {}", message.data);
                                            None
                                        } else {
                                            Some(Err(StreamingError::Parsing(
                                                format!("Failed to parse SSE event: {} (raw: {})", e, message.data)
                                            )))
                                        }
                                    }
                                }
                            } else {
                                // Not valid JSON at all
                                Some(Err(StreamingError::Parsing(
                                    format!("SSE event is not valid JSON: {}", message.data)
                                )))
                            }
                        }
                        Err(e) => {
                            // Check if this is a normal stream end vs actual error
                            match e {
                                reqwest_eventsource::Error::StreamEnded => {
                                    debug!("SSE stream completed normally");
                                    None // Normal stream end, not an error
                                }
                                reqwest_eventsource::Error::InvalidStatusCode(status, response) => {
                                    // We have access to the response body for error details
                                    let status_code = status.as_u16();

                                    // Read the response body to get error details
                                    let error_body = match response.text().await {
                                        Ok(body) => body,
                                        Err(_) => "Failed to read error response body".to_string()
                                    };

                                    error!("SSE connection error - HTTP {}: {}", status_code, error_body);

                                    let detailed_error = format!(
                                        "HTTP {} error: {}",
                                        status_code,
                                        error_body
                                    );

                                    Some(Err(StreamingError::Connection(detailed_error)))
                                }
                                _ => {
                                    let error_str = e.to_string();
                                    if error_str.contains("stream closed") {
                                        debug!("SSE stream closed");
                                        None
                                    } else {
                                        error!("SSE connection error: {}", e);
                                        Some(Err(StreamingError::Connection(error_str)))
                                    }
                                }
                            }
                        }
                    }
                });

                Ok(stream)
            }
        })
    }

    /// Generate reconnection utilities
    fn generate_reconnection_utilities(
        &self,
        reconnect_config: &crate::streaming::ReconnectionConfig,
    ) -> Result<TokenStream> {
        let max_retries = reconnect_config.max_retries;
        let initial_delay = reconnect_config.initial_delay_ms;
        let max_delay = reconnect_config.max_delay_ms;
        let backoff_multiplier = reconnect_config.backoff_multiplier;

        Ok(quote! {
            /// Reconnection configuration and utilities
            #[derive(Debug, Clone)]
            pub struct ReconnectionManager {
                max_retries: u32,
                initial_delay_ms: u64,
                max_delay_ms: u64,
                backoff_multiplier: f64,
                current_attempt: u32,
            }

            impl ReconnectionManager {
                /// Create a new reconnection manager
                pub fn new() -> Self {
                    Self {
                        max_retries: #max_retries,
                        initial_delay_ms: #initial_delay,
                        max_delay_ms: #max_delay,
                        backoff_multiplier: #backoff_multiplier,
                        current_attempt: 0,
                    }
                }

                /// Check if we should retry the connection
                pub fn should_retry(&self) -> bool {
                    self.current_attempt < self.max_retries
                }

                /// Get the delay for the next retry attempt
                pub fn next_retry_delay(&mut self) -> Duration {
                    if !self.should_retry() {
                        return Duration::from_secs(0);
                    }

                    let delay_ms = (self.initial_delay_ms as f64
                        * self.backoff_multiplier.powi(self.current_attempt as i32)) as u64;
                    let delay_ms = delay_ms.min(self.max_delay_ms);

                    self.current_attempt += 1;
                    Duration::from_millis(delay_ms)
                }

                /// Reset the retry counter after a successful connection
                pub fn reset(&mut self) {
                    self.current_attempt = 0;
                }

                /// Get the current attempt number
                pub fn current_attempt(&self) -> u32 {
                    self.current_attempt
                }
            }

            impl Default for ReconnectionManager {
                fn default() -> Self {
                    Self::new()
                }
            }
        })
    }
}
