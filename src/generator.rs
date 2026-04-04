use crate::{GeneratorError, Result, analysis::SchemaAnalysis, streaming::StreamingConfig};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Info about schemas that are variants in discriminated unions
#[derive(Clone)]
struct DiscriminatedVariantInfo {
    /// The discriminator field name (e.g., "type")
    discriminator_field: String,
    /// Whether the parent union is untagged
    is_parent_untagged: bool,
}

#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// Path to OpenAPI specification file
    pub spec_path: PathBuf,
    /// Output directory for generated code (e.g., "src/gen")
    pub output_dir: PathBuf,
    /// Name of the generated module
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
            schema_extensions: Vec::new(),
            http_client_config: None,
            retry_config: None,
            tracing_enabled: true,
            auth_config: None,
            enable_registry: false,
            registry_only: false,
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
}

pub struct CodeGenerator {
    config: GeneratorConfig,
}

impl CodeGenerator {
    pub fn new(config: GeneratorConfig) -> Self {
        Self { config }
    }

    /// Get reference to the generator configuration
    pub fn config(&self) -> &GeneratorConfig {
        &self.config
    }

    /// Generate all files for the API
    pub fn generate_all(&self, analysis: &mut SchemaAnalysis) -> Result<GenerationResult> {
        let mut files = Vec::new();

        if !self.config.registry_only {
            // Generate types file
            let types_content = self.generate_types(analysis)?;
            files.push(GeneratedFile {
                path: "types.rs".into(),
                content: types_content,
            });

            // Generate streaming client if configured
            if let Some(ref streaming_config) = self.config.streaming_config {
                let streaming_content =
                    self.generate_streaming_client(streaming_config, analysis)?;
                files.push(GeneratedFile {
                    path: "streaming.rs".into(),
                    content: streaming_content,
                });
            }

            // Generate HTTP client if enabled
            if self.config.enable_async_client {
                let http_content = self.generate_http_client(analysis)?;
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

        // Generate mod.rs file
        let mod_content = self.generate_mod_file(&files)?;
        let mod_file = GeneratedFile {
            path: "mod.rs".into(),
            content: mod_content,
        };

        Ok(GenerationResult { files, mod_file })
    }

    /// Generate just the types (legacy single-file interface)
    pub fn generate(&self, analysis: &mut SchemaAnalysis) -> Result<String> {
        self.generate_types(analysis)
    }

    /// Generate the types.rs file content
    fn generate_types(&self, analysis: &mut SchemaAnalysis) -> Result<String> {
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
                                        is_parent_untagged,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        // Generate types based on dependency order
        let generation_order = analysis.dependencies.topological_sort()?;

        // Generate all schemas, including those not in dependency graph
        let mut processed = std::collections::HashSet::new();

        // First, generate schemas in dependency order
        for schema_name in generation_order {
            if let Some(schema) = analysis.schemas.get(&schema_name) {
                let type_def =
                    self.generate_type_definition(schema, analysis, &discriminated_variant_info)?;
                if !type_def.is_empty() {
                    type_definitions.extend(type_def);
                }
                processed.insert(schema_name);
            }
        }

        // Then generate any remaining schemas not in dependency graph
        // Sort by name for deterministic output
        let mut remaining_schemas: Vec<_> = analysis
            .schemas
            .iter()
            .filter(|(name, _)| !processed.contains(*name))
            .collect();
        remaining_schemas.sort_by_key(|(name, _)| name.as_str());

        for (_schema_name, schema) in remaining_schemas {
            let type_def =
                self.generate_type_definition(schema, analysis, &discriminated_variant_info)?;
            if !type_def.is_empty() {
                type_definitions.extend(type_def);
            }
        }

        // Generate file with imports and types (no module wrapper)
        let generated = quote! {
            //! Generated types from OpenAPI specification
            //!
            //! This file contains all the generated types for the API.
            //! Do not edit manually - regenerate using the appropriate script.

            #![allow(clippy::large_enum_variant)]
            #![allow(clippy::format_in_format_args)]
            #![allow(clippy::let_unit_value)]
            #![allow(unreachable_patterns)]

            use serde::{Deserialize, Serialize};

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

        // Generate imports
        let imports = quote! {
            //! Generated streaming client for SSE (Server-Sent Events)
            //!
            //! This file contains the streaming client implementation.
            //! Do not edit manually - regenerate using the appropriate script.
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

    /// Generate HTTP client code for regular (non-streaming) requests
    pub fn generate_http_client(&self, analysis: &SchemaAnalysis) -> Result<String> {
        let error_types = self.generate_http_error_types();
        let client_struct = self.generate_http_client_struct();
        let operation_methods = self.generate_operation_methods(analysis);

        let generated = quote! {
            //! Generated HTTP client for regular API requests
            //!
            //! This file contains the HTTP client implementation for GET, POST, etc.
            //! Do not edit manually - regenerate using the appropriate script.
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

    /// Generate HTTP error type and result alias
    fn generate_http_error_types(&self) -> TokenStream {
        quote! {
            use thiserror::Error;

            /// HTTP client errors that can occur during API requests
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

                /// Response deserialization error
                #[error("Failed to deserialize response: {0}")]
                Deserialization(String),

                /// HTTP error response (4xx, 5xx)
                #[error("HTTP error {status}: {message}")]
                Http {
                    status: u16,
                    message: String,
                    body: Option<String>,
                },

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
                /// Create an HTTP error from a status code and message
                pub fn from_status(status: u16, message: impl Into<String>, body: Option<String>) -> Self {
                    Self::Http {
                        status,
                        message: message.into(),
                        body,
                    }
                }

                /// Create a serialization error
                pub fn serialization_error(error: impl std::fmt::Display) -> Self {
                    Self::Serialization(error.to_string())
                }

                /// Create a deserialization error
                pub fn deserialization_error(error: impl std::fmt::Display) -> Self {
                    Self::Deserialization(error.to_string())
                }

                /// Check if this is a client error (4xx)
                pub fn is_client_error(&self) -> bool {
                    matches!(self, Self::Http { status, .. } if *status >= 400 && *status < 500)
                }

                /// Check if this is a server error (5xx)
                pub fn is_server_error(&self) -> bool {
                    matches!(self, Self::Http { status, .. } if *status >= 500 && *status < 600)
                }

                /// Check if this error is retryable
                pub fn is_retryable(&self) -> bool {
                    match self {
                        Self::Network(_) => true,
                        Self::Middleware(_) => true,
                        Self::Timeout => true,
                        Self::Http { status, .. } => {
                            // Retry on 429 (rate limit), 500, 502, 503, 504
                            matches!(status, 429 | 500 | 502 | 503 | 504)
                        }
                        _ => false,
                    }
                }
            }

            /// Result type for HTTP operations
            pub type HttpResult<T> = Result<T, HttpError>;
        }
    }

    /// Generate mod.rs file that exports all modules
    fn generate_mod_file(&self, files: &[GeneratedFile]) -> Result<String> {
        let mut module_declarations = Vec::new();
        let mut pub_uses = Vec::new();

        for file in files {
            if let Some(module_name) = file.path.file_stem().and_then(|s| s.to_str()) {
                if module_name != "mod" {
                    module_declarations.push(format!("pub mod {module_name};"));
                    pub_uses.push(format!("pub use {module_name}::*;"));
                }
            }
        }

        let content = format!(
            r#"//! Generated API modules
//!
//! This module exports all generated API types and clients.
//! Do not edit manually - regenerate using the appropriate script.

#![allow(unused_imports)]

{}

{}
"#,
            module_declarations.join("\n"),
            pub_uses.join("\n")
        );

        Ok(content)
    }

    /// Helper method to write all generated files to disk
    pub fn write_files(&self, result: &GenerationResult) -> Result<()> {
        use std::fs;

        // Create output directory if it doesn't exist
        fs::create_dir_all(&self.config.output_dir)?;

        // Write all files
        for file in &result.files {
            let file_path = self.config.output_dir.join(&file.path);
            fs::write(&file_path, &file.content)?;
        }

        // Write mod.rs
        let mod_path = self.config.output_dir.join(&result.mod_file.path);
        fs::write(&mod_path, &result.mod_file.content)?;

        Ok(())
    }

    fn generate_type_definition(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        analysis: &crate::analysis::SchemaAnalysis,
        discriminated_variant_info: &BTreeMap<String, DiscriminatedVariantInfo>,
    ) -> Result<TokenStream> {
        use crate::analysis::SchemaType;

        match &schema.schema_type {
            SchemaType::Primitive { rust_type } => {
                // Generate type alias for primitives that are referenced by other schemas
                self.generate_type_alias(schema, rust_type)
            }
            SchemaType::StringEnum { values } => self.generate_string_enum(schema, values),
            SchemaType::ExtensibleEnum { known_values } => {
                self.generate_extensible_enum(schema, known_values)
            }
            SchemaType::Object {
                properties,
                required,
                additional_properties,
            } => self.generate_struct(
                schema,
                properties,
                required,
                *additional_properties,
                analysis,
                discriminated_variant_info.get(&schema.name),
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
                    self.generate_union_enum(schema, &schema_refs)
                } else {
                    self.generate_discriminated_enum(
                        schema,
                        discriminator_field,
                        variants,
                        analysis,
                    )
                }
            }
            SchemaType::Union { variants } => self.generate_union_enum(schema, variants),
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
                // Generate type alias for named array schemas
                let array_name = format_ident!("{}", self.to_rust_type_name(&schema.name));
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

        // Parse the rust type into tokens
        let base_type = if rust_type.contains("::") {
            let parts: Vec<&str> = rust_type.split("::").collect();
            if parts.len() == 2 {
                let module = format_ident!("{}", parts[0]);
                let type_name_part = format_ident!("{}", parts[1]);
                quote! { #module::#type_name_part }
            } else {
                // More complex path
                let path_parts: Vec<_> = parts.iter().map(|p| format_ident!("{}", p)).collect();
                quote! { #(#path_parts)::* }
            }
        } else {
            let simple_type = format_ident!("{}", rust_type);
            quote! { #simple_type }
        };

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
    ) -> Result<TokenStream> {
        let enum_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // For extensible enums, we need a different approach:
        // 1. Create a regular enum with known variants + Custom
        // 2. Implement custom serialization/deserialization

        let known_variants = known_values.iter().map(|value| {
            let variant_name = self.to_rust_enum_variant(value);
            let variant_ident = format_ident!("{}", variant_name);
            quote! {
                #variant_ident,
            }
        });

        let match_arms_de = known_values.iter().map(|value| {
            let variant_name = self.to_rust_enum_variant(value);
            let variant_ident = format_ident!("{}", variant_name);
            quote! {
                #value => Ok(#enum_name::#variant_ident),
            }
        });

        let match_arms_ser = known_values.iter().map(|value| {
            let variant_name = self.to_rust_enum_variant(value);
            let variant_ident = format_ident!("{}", variant_name);
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
    ) -> Result<TokenStream> {
        let enum_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        // Determine which variant should be the default
        let default_value = schema
            .default
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let variants = values.iter().enumerate().map(|(i, value)| {
            // Convert string value to valid Rust enum variant (PascalCase)
            let variant_name = self.to_rust_enum_variant(value);
            let variant_ident = format_ident!("{}", variant_name);

            // Check if this variant should be the default
            let is_default = if let Some(ref default) = default_value {
                value == default
            } else {
                i == 0 // Fall back to first variant if no default specified
            };

            if is_default {
                quote! {
                    #[default]
                    #[serde(rename = #value)]
                    #variant_ident,
                }
            } else {
                quote! {
                    #[serde(rename = #value)]
                    #variant_ident,
                }
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
                #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
                #[cfg_attr(feature = "specta", derive(specta::Type))]
            }
        } else {
            quote! {
                #[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
            }
        };

        Ok(quote! {
            #doc_comment
            #derives
            pub enum #enum_name {
                #(#variants)*
            }
        })
    }

    fn generate_struct(
        &self,
        schema: &crate::analysis::AnalyzedSchema,
        properties: &BTreeMap<String, crate::analysis::PropertyInfo>,
        required: &std::collections::HashSet<String>,
        additional_properties: bool,
        analysis: &crate::analysis::SchemaAnalysis,
        discriminator_info: Option<&DiscriminatedVariantInfo>,
    ) -> Result<TokenStream> {
        let struct_name = format_ident!("{}", self.to_rust_type_name(&schema.name));

        // Sort properties by name for deterministic output
        let mut sorted_properties: Vec<_> = properties.iter().collect();
        sorted_properties.sort_by_key(|(name, _)| name.as_str());

        let mut fields: Vec<TokenStream> = sorted_properties
            .into_iter()
            .filter(|(field_name, _)| {
                // Skip the discriminator field ONLY if:
                // 1. This struct is a variant in a discriminated union, AND
                // 2. The parent union is tagged (not untagged)
                if let Some(info) = discriminator_info {
                    if !info.is_parent_untagged
                        && field_name.as_str() == info.discriminator_field.as_str()
                    {
                        false // Skip the field
                    } else {
                        true // Keep the field
                    }
                } else {
                    true // No discriminator info, keep all fields
                }
            })
            .map(|(field_name, prop)| {
                let field_ident = Self::to_field_ident(&self.to_rust_field_name(field_name));
                let is_required = required.contains(field_name);
                let field_type =
                    self.generate_field_type(&schema.name, field_name, prop, is_required, analysis);

                let serde_attrs = self.generate_serde_field_attrs(field_name, prop, is_required);
                let specta_attrs = self.generate_specta_field_attrs(field_name);

                let doc_comment = if let Some(desc) = &prop.description {
                    let sanitized_desc = self.sanitize_doc_comment(desc);
                    quote! { #[doc = #sanitized_desc] }
                } else {
                    TokenStream::new()
                };

                quote! {
                    #doc_comment
                    #serde_attrs
                    #specta_attrs
                    pub #field_ident: #field_type,
                }
            })
            .collect();

        // Add additional properties field if enabled
        if additional_properties {
            fields.push(quote! {
                /// Additional properties not explicitly defined in the schema
                #[serde(flatten)]
                pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>,
            });
        }

        let doc_comment = if let Some(desc) = &schema.description {
            quote! { #[doc = #desc] }
        } else {
            TokenStream::new()
        };

        // Generate derives with optional Specta support
        // Note: We use snake_case everywhere (matching the OpenAPI spec) for consistency
        // between Rust, JSON API, and TypeScript
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
            return self.generate_union_enum(schema, &schema_refs);
        }

        let enum_variants = variants.iter().map(|variant| {
            let variant_name = format_ident!("{}", variant.rust_name);
            let variant_value = &variant.discriminator_value;

            // Always use tuple variant that references the existing type
            // This ensures the standalone event types are actually used
            let variant_type = format_ident!("{}", self.to_rust_type_name(&variant.type_name));
            quote! {
                #[serde(rename = #variant_value)]
                #variant_name(#variant_type),
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
            } else if variant.target.starts_with("Vec<") && variant.target.ends_with(">") {
                // Handle Vec types by parsing the inner type
                let inner = &variant.target[4..variant.target.len() - 1];

                // Handle nested Vec types (e.g., Vec<Vec<i64>>)
                if inner.starts_with("Vec<") && inner.ends_with(">") {
                    let inner_inner = &inner[4..inner.len() - 1];
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
            } else {
                let type_ident = format_ident!("{}", self.to_rust_type_name(&variant.target));
                quote! { #type_ident }
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

    fn generate_field_type(
        &self,
        schema_name: &str,
        field_name: &str,
        prop: &crate::analysis::PropertyInfo,
        is_required: bool,
        analysis: &crate::analysis::SchemaAnalysis,
    ) -> TokenStream {
        use crate::analysis::SchemaType;

        let base_type = match &prop.schema_type {
            SchemaType::Primitive { rust_type } => {
                // Handle complex types like serde_json::Value
                if rust_type.contains("::") {
                    let parts: Vec<&str> = rust_type.split("::").collect();
                    if parts.len() == 2 {
                        let module = format_ident!("{}", parts[0]);
                        let type_name = format_ident!("{}", parts[1]);
                        quote! { #module::#type_name }
                    } else {
                        // More than 2 parts, construct path
                        let path_parts: Vec<_> =
                            parts.iter().map(|p| format_ident!("{}", p)).collect();
                        quote! { #(#path_parts)::* }
                    }
                } else {
                    let type_ident = format_ident!("{}", rust_type);
                    quote! { #type_ident }
                }
            }
            SchemaType::Reference { target } => {
                let target_type = format_ident!("{}", self.to_rust_type_name(target));
                // Wrap recursive references in Box<T> for heap allocation
                if analysis.dependencies.recursive_schemas.contains(target) {
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
        };

        // Check if this field has a nullable override
        let override_key = format!("{schema_name}.{field_name}");
        let is_nullable_override = self
            .config
            .nullable_field_overrides
            .get(&override_key)
            .copied()
            .unwrap_or(false);

        if is_required && !prop.nullable && !is_nullable_override {
            base_type
        } else {
            quote! { Option<#base_type> }
        }
    }

    fn generate_serde_field_attrs(
        &self,
        field_name: &str,
        prop: &crate::analysis::PropertyInfo,
        is_required: bool,
    ) -> TokenStream {
        let mut attrs = Vec::new();

        // Generate rename attribute if field name differs from Rust identifier
        // Strip r# prefix for comparison since serde handles raw idents transparently
        let rust_field_name = self.to_rust_field_name(field_name);
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

        // Only add default attribute for required fields that have default values
        // Optional fields (Option<T>) already default to None, so don't need #[serde(default)]
        if prop.default.is_some() && (is_required && !prop.nullable) {
            attrs.push(quote! { default });
        }

        if attrs.is_empty() {
            TokenStream::new()
        } else {
            quote! { #[serde(#(#attrs),*)] }
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

    fn to_rust_enum_variant(&self, s: &str) -> String {
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
            result = format!("Variant{result}");
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

        result
    }

    fn to_rust_field_name(&self, s: &str) -> String {
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
            result = format!("field_{result}");
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
                crate::analysis::SchemaType::Reference { target } => {
                    if !defined_types.contains(target) {
                        missing.insert(target.clone());
                    }
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
            SchemaType::Primitive { rust_type } => {
                // Handle complex types like serde_json::Value
                if rust_type.contains("::") {
                    let parts: Vec<&str> = rust_type.split("::").collect();
                    if parts.len() == 2 {
                        let module = format_ident!("{}", parts[0]);
                        let type_name = format_ident!("{}", parts[1]);
                        quote! { #module::#type_name }
                    } else {
                        // More than 2 parts, construct path
                        let path_parts: Vec<_> =
                            parts.iter().map(|p| format_ident!("{}", p)).collect();
                        quote! { #(#path_parts)::* }
                    }
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
