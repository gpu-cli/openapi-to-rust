//! HTTP client generation for OpenAPI specifications.
//!
//! This module is part of the code generator that creates production-ready HTTP clients
//! from OpenAPI specifications. It generates clients with middleware support including
//! retry logic and request tracing.
//!
//! # Overview
//!
//! The client generator creates:
//! - `HttpClient` struct with middleware stack (reqwest-middleware)
//! - Retry logic with exponential backoff (reqwest-retry)
//! - Request/response tracing (reqwest-tracing)
//! - Direct methods for all API operations (GET, POST, PUT, DELETE, PATCH)
//! - Comprehensive error handling with [`HttpError`](crate::http_error::HttpError)
//! - Builder pattern for configuration
//!
//! # Generated Code Structure
//!
//! For each OpenAPI specification, the generator creates:
//!
//! ```rust,ignore
//! // Generated client.rs file
//!
//! use crate::types::*;
//! use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
//! use std::collections::BTreeMap;
//!
//! pub struct HttpClient {
//!     base_url: String,
//!     api_key: Option<String>,
//!     http_client: ClientWithMiddleware,
//!     custom_headers: BTreeMap<String, String>,
//! }
//!
//! impl HttpClient {
//!     pub fn new() -> Self { /* ... */ }
//!     pub fn with_config(retry_config: Option<RetryConfig>, enable_tracing: bool) -> Self { /* ... */ }
//!     pub fn with_base_url(self, base_url: String) -> Self { /* ... */ }
//!     pub fn with_api_key(self, api_key: String) -> Self { /* ... */ }
//!     pub fn with_header(self, key: String, value: String) -> Self { /* ... */ }
//!
//!     // Generated operation methods
//!     pub async fn list_items(&self) -> Result<ItemList, HttpError> { /* ... */ }
//!     pub async fn create_item(&self, request: CreateItemRequest) -> Result<Item, HttpError> { /* ... */ }
//!     pub async fn get_item(&self, id: impl AsRef<str>) -> Result<Item, HttpError> { /* ... */ }
//! }
//! ```
//!
//! # Middleware Stack
//!
//! The generated client uses `reqwest-middleware` to build a composable middleware stack:
//!
//! 1. **Tracing Middleware** (optional, enabled by default)
//!    - Logs HTTP requests/responses
//!    - Creates spans for distributed tracing
//!    - Integrates with `tracing` ecosystem
//!
//! 2. **Retry Middleware** (optional, configured via TOML)
//!    - Exponential backoff retry policy
//!    - Automatically retries transient errors (429, 500, 502, 503, 504)
//!    - Configurable max retries and delay bounds
//!
//! # Configuration
//!
//! ## Via TOML
//!
//! ```toml
//! [http_client]
//! base_url = "https://api.example.com"
//! timeout_seconds = 30
//!
//! [http_client.retry]
//! max_retries = 3
//! initial_delay_ms = 500
//! max_delay_ms = 16000
//!
//! [http_client.tracing]
//! enabled = true
//! ```
//!
//! ## Via Rust API
//!
//! ```no_run
//! use openapi_to_rust::{GeneratorConfig, http_config::*};
//! use std::path::PathBuf;
//!
//! let config = GeneratorConfig {
//!     spec_path: PathBuf::from("openapi.json"),
//!     enable_async_client: true,
//!     retry_config: Some(RetryConfig {
//!         max_retries: 3,
//!         initial_delay_ms: 500,
//!         max_delay_ms: 16000,
//!     }),
//!     tracing_enabled: true,
//!     // ... other fields
//!     ..Default::default()
//! };
//! ```
//!
//! # Generated Client Usage
//!
//! ```rust,ignore
//! use crate::generated::client::HttpClient;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create client with retry and tracing
//!     let client = HttpClient::new()
//!         .with_base_url("https://api.example.com".to_string())
//!         .with_api_key("your-api-key".to_string())
//!         .with_header("X-Custom-Header".to_string(), "value".to_string());
//!
//!     // Make API calls - retries happen automatically
//!     let items = client.list_items().await?;
//!     println!("Found {} items", items.items.len());
//!
//!     Ok(())
//! }
//! ```
//!
//! # HTTP Method Support
//!
//! The generator supports all standard HTTP methods:
//! - `GET` - List and retrieve operations
//! - `POST` - Create operations
//! - `PUT` - Full update operations
//! - `PATCH` - Partial update operations
//! - `DELETE` - Delete operations
//!
//! # Error Handling
//!
//! All generated methods return `Result<T, HttpError>` where `HttpError` provides:
//! - Detailed error information
//! - Retry detection via `is_retryable()`
//! - Error categorization (client errors, server errors)
//!
//! See [`http_error`](crate::http_error) module for details.
//!
//! # Implementation Details
//!
//! The generator uses the following approach:
//! 1. Analyzes OpenAPI operations to extract HTTP methods, paths, parameters
//! 2. Generates typed request/response handling
//! 3. Creates method signatures with proper parameter types
//! 4. Generates path parameter substitution
//! 5. Handles query parameters and request bodies
//! 6. Configures middleware stack based on generator config

use crate::analysis::{
    OperationInfo, OperationRepresentation, OperationResponseBody, ParameterInfo, SchemaAnalysis,
};
use crate::generator::CodeGenerator;
use heck::{ToPascalCase, ToSnakeCase};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;

struct AllocatedOperationParam<'a> {
    param: &'a ParameterInfo,
    ident: syn::Ident,
}

#[derive(Clone)]
struct BodyFieldPlan {
    wire_name: String,
    preferred_method_name: String,
    value_ident: syn::Ident,
    value_type: TokenStream,
    access_path: Vec<syn::Ident>,
    tri_state: bool,
}

#[derive(Clone, Copy)]
enum MultipartClientFieldKind {
    RawBytes,
    Base64,
    Base64UrlUnpadded,
    Text,
}

#[derive(Clone, Copy)]
enum ClientSuccessBody<'a> {
    Json(&'a str),
    Text,
    Binary,
    EventStream,
    ParsedSse,
    Empty,
}

#[derive(Clone)]
struct ClientSuccessSelection<'a> {
    statuses: Vec<&'a str>,
    excluded_statuses: Vec<&'a str>,
    body: ClientSuccessBody<'a>,
    accept: Option<&'a str>,
}

/// Final flat client binding shared by rendering, builders and metadata.
pub(crate) struct ClientMethodPlan<'a> {
    pub operation: &'a OperationInfo,
    pub method_ident: syn::Ident,
    pub request_parameters: TokenStream,
    pub generics: TokenStream,
    pub response_type: TokenStream,
    pub error_type: TokenStream,
    pub response: ClientResponsePlan,
    pub multipart_filenames: Option<syn::Ident>,
    pub filenames_method_ident: Option<syn::Ident>,
    pub builder_method_ident: Option<syn::Ident>,
    pub builder_type_ident: Option<syn::Ident>,
    success: ClientSuccessSelection<'a>,
    validate_content_type: bool,
}

pub(crate) struct ClientResponsePlan {
    pub statuses: Vec<String>,
    pub excluded_statuses: Vec<String>,
    pub media_type: Option<String>,
    pub kind: String,
    pub consumption: String,
}

enum RequiredBodyConstruction {
    Default,
    New(Vec<BodyConstructorParam>),
    Whole,
}

struct BodyConstructorParam {
    preferred_ident: syn::Ident,
    value_type: TokenStream,
}

struct BodyModelPlan {
    body_ident: syn::Ident,
    body_type: TokenStream,
    required_construction: RequiredBodyConstruction,
    optional_fields: Vec<BodyFieldPlan>,
}

impl CodeGenerator {
    /// Generate the HTTP client struct with middleware support
    pub fn generate_http_client_struct(&self) -> TokenStream {
        let has_retry = self.config().retry_config.is_some();
        let has_tracing = self.config().tracing_enabled;

        // Generate RetryConfig struct if needed
        let retry_config_struct = if has_retry {
            quote! {
                /// Retry configuration for HTTP requests
                #[derive(Debug, Clone)]
                pub struct RetryConfig {
                    pub max_retries: u32,
                    pub initial_delay_ms: u64,
                    pub max_delay_ms: u64,
                }

                impl Default for RetryConfig {
                    fn default() -> Self {
                        Self {
                            max_retries: 3,
                            initial_delay_ms: 500,
                            max_delay_ms: 16000,
                        }
                    }
                }
            }
        } else {
            quote! {}
        };

        // Generate the main HttpClient struct
        let client_struct = quote! {
            use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
            use std::collections::BTreeMap;

            /// Default upper bound for any response body buffered in memory.
            pub const DEFAULT_MAX_RESPONSE_BODY_BYTES: usize = 8 * 1024 * 1024;

            /// HTTP client for making API requests
            #[derive(Clone)]
            pub struct HttpClient {
                base_url: String,
                api_key: Option<String>,
                http_client: ClientWithMiddleware,
                custom_headers: BTreeMap<String, String>,
                max_response_body_bytes: usize,
            }

            // Buffer a response body while enforcing `limit`, using
            // `bytes_stream()` rather than `Response::chunk()`: `chunk()` is
            // native-only in reqwest and does not exist on the wasm32 backend,
            // so it broke every generated client under `trunk serve`
            // (openapi-generator-xhz, issue #74). `bytes_stream()` is available
            // on both targets behind reqwest's `stream` feature.
            async fn __read_bounded_response_body(
                response: reqwest::Response,
                limit: usize,
            ) -> Result<Vec<u8>, HttpError> {
                use futures_util::StreamExt;

                let mut body = Vec::new();
                let mut stream = std::pin::pin!(response.bytes_stream());
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(HttpError::Network)?;
                    let next_len = body.len().checked_add(chunk.len());
                    if next_len.is_none_or(|next_len| next_len > limit) {
                        return Err(HttpError::ResponseTooLarge { limit });
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(body)
            }
        };

        // Generate constructor
        let constructor = self.generate_constructor(has_retry, has_tracing);

        // Generate builder methods
        let builder_methods = self.generate_builder_methods();

        // Generate Default implementation
        let default_impl = quote! {
            impl Default for HttpClient {
                fn default() -> Self {
                    Self::new()
                }
            }
        };

        // Path-segment percent encoder, used by url construction (T5).
        // Encodes per RFC3986 §3.3: only ALPHA, DIGIT, and `-._~` pass through;
        // everything else becomes `%XX`.
        let path_encoder = quote! {
            fn __pct_encode_path_segment(s: &str) -> String {
                let mut out = String::with_capacity(s.len());
                for &b in s.as_bytes() {
                    match b {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                            out.push(b as char);
                        }
                        _ => {
                            out.push('%');
                            out.push_str(&format!("{:02X}", b));
                        }
                    }
                }
                out
            }
        };

        // Combine all parts
        quote! {
            #retry_config_struct
            #client_struct

            impl HttpClient {
                #constructor
                #builder_methods
            }

            #default_impl
            #path_encoder
        }
    }

    /// Generate the constructor method
    fn generate_constructor(&self, has_retry: bool, has_tracing: bool) -> TokenStream {
        // Seed `base_url` from configuration rather than always starting empty.
        // A client built with an empty base URL sends every request to a
        // relative path and fails, so a user who set `[http_client] base_url`
        // in their TOML — or whose spec declares `servers[0].url`, which the
        // config layer resolves into the same field — previously had to repeat
        // it via `with_base_url` or watch every call 404 (openapi-generator-igg).
        let configured_base_url = self
            .config()
            .http_client_config
            .as_ref()
            .and_then(|http| http.base_url.as_deref())
            .unwrap_or_default();
        let default_base_url = quote! { #configured_base_url.to_string() };
        let max_response_body_bytes = self
            .config()
            .http_client_config
            .as_ref()
            .and_then(|http| http.max_response_body_bytes)
            .unwrap_or(8 * 1024 * 1024);

        let retry_param = if has_retry {
            quote! { retry_config: Option<RetryConfig>, }
        } else {
            quote! {}
        };

        let tracing_param = if has_tracing {
            quote! { enable_tracing: bool, }
        } else {
            quote! {}
        };

        let retry_middleware = if has_retry {
            quote! {
                if let Some(config) = retry_config {
                    use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};

                    let retry_policy = ExponentialBackoff::builder()
                        .retry_bounds(
                            std::time::Duration::from_millis(config.initial_delay_ms),
                            std::time::Duration::from_millis(config.max_delay_ms),
                        )
                        .build_with_max_retries(config.max_retries);

                    let retry_middleware = RetryTransientMiddleware::new_with_policy(retry_policy);
                    client_builder = client_builder.with(retry_middleware);
                }
            }
        } else {
            quote! {}
        };

        let tracing_middleware = if has_tracing {
            quote! {
                if enable_tracing {
                    use reqwest_tracing::TracingMiddleware;
                    client_builder = client_builder.with(TracingMiddleware::default());
                }
            }
        } else {
            quote! {}
        };

        let default_constructor = if has_retry && has_tracing {
            quote! {
                /// Create a new HTTP client with default configuration
                pub fn new() -> Self {
                    Self::with_config(None, true)
                }
            }
        } else if has_retry {
            quote! {
                /// Create a new HTTP client with default configuration
                pub fn new() -> Self {
                    Self::with_config(None)
                }
            }
        } else if has_tracing {
            quote! {
                /// Create a new HTTP client with default configuration
                pub fn new() -> Self {
                    Self::with_config(true)
                }
            }
        } else {
            quote! {
                /// Create a new HTTP client with default configuration
                pub fn new() -> Self {
                    let reqwest_client = reqwest::Client::new();
                    let client_builder = ClientBuilder::new(reqwest_client);
                    let http_client = client_builder.build();

                    Self {
                        base_url: #default_base_url,
                        api_key: None,
                        http_client,
                        custom_headers: BTreeMap::new(),
                        max_response_body_bytes: #max_response_body_bytes,
                    }
                }
            }
        };

        if has_retry || has_tracing {
            quote! {
                #default_constructor

                /// Create a new HTTP client with custom configuration
                pub fn with_config(#retry_param #tracing_param) -> Self {
                    let reqwest_client = reqwest::Client::new();
                    let mut client_builder = ClientBuilder::new(reqwest_client);

                    #tracing_middleware
                    #retry_middleware

                    let http_client = client_builder.build();

                    Self {
                        base_url: #default_base_url,
                        api_key: None,
                        http_client,
                        custom_headers: BTreeMap::new(),
                        max_response_body_bytes: #max_response_body_bytes,
                    }
                }
            }
        } else {
            default_constructor
        }
    }

    /// Generate builder methods for configuration
    fn generate_builder_methods(&self) -> TokenStream {
        quote! {
            /// Set the base URL for all requests
            pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
                self.base_url = base_url.into();
                self
            }

            /// Set the API key for authentication
            pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
                self.api_key = Some(api_key.into());
                self
            }

            /// Set the maximum number of response-body bytes buffered in memory.
            pub fn with_max_response_body_bytes(mut self, limit: usize) -> Self {
                self.max_response_body_bytes = limit;
                self
            }

            /// Add a custom header to all requests
            pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
                self.custom_headers.insert(name.into(), value.into());
                self
            }

            /// Add multiple custom headers
            pub fn with_headers(mut self, headers: BTreeMap<String, String>) -> Self {
                self.custom_headers.extend(headers);
                self
            }
        }
    }

    /// Generate HTTP operation methods for the client.
    ///
    /// Emits per-operation typed error enums (one variant per declared non-2xx
    /// response with a body schema) BEFORE the `impl HttpClient` block so the
    /// generated method signatures can reference them. This low-level helper
    /// intentionally emits every analyzed operation; use
    /// [`Self::generate_http_client`] or [`Self::generate_all`] to honor the
    /// configured `[client].operations` scope.
    /// Generate standalone HTTP methods. Declared SSE alternatives return raw
    /// byte streams; `generate_all` bundles the optional parsed SSE runtime.
    pub fn generate_operation_methods(&self, analysis: &SchemaAnalysis) -> TokenStream {
        let operations: Vec<&OperationInfo> = analysis.operations.values().collect();
        let mut config = self.config().clone();
        config.enable_sse_client = false;
        CodeGenerator::new(config).generate_operation_methods_for(analysis, &operations)
    }

    /// Generate every operation-owned client artifact from one resolved
    /// operation slice. This keeps methods, parameter enums, and typed error
    /// enums in lockstep for selective clients.
    pub(crate) fn generate_operation_methods_for(
        &self,
        analysis: &SchemaAnalysis,
        operations: &[&OperationInfo],
    ) -> TokenStream {
        let param_enums = self.generate_param_enum_types(operations);

        let op_error_enums: Vec<TokenStream> = operations
            .iter()
            .copied()
            .filter_map(|op| self.generate_op_error_enum(op))
            .collect();

        let plans = self.client_method_plans(analysis, operations);
        let methods: Vec<TokenStream> = plans
            .iter()
            .map(|plan| self.generate_planned_operation_method(analysis, plan))
            .collect();
        let (operation_builders, builder_entries) =
            self.generate_operation_builders(analysis, &plans);

        quote! {
            #param_enums

            #(#op_error_enums)*

            #(#operation_builders)*

            impl HttpClient {
                #(#methods)*
                #(#builder_entries)*
            }
        }
    }

    pub(crate) fn client_method_plans<'a>(
        &self,
        analysis: &'a SchemaAnalysis,
        operations: &[&'a OperationInfo],
    ) -> Vec<ClientMethodPlan<'a>> {
        // Reserve every real operation before allocating additive bindings.
        let mut used: std::collections::HashSet<String> = [
            "new",
            "with_config",
            "with_base_url",
            "with_api_key",
            "with_max_response_body_bytes",
            "with_header",
            "with_headers",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let inherent = used.clone();
        for operation in operations {
            let ident = self.get_method_name(operation).to_string();
            used.insert(ident.strip_prefix("r#").unwrap_or(&ident).to_string());
        }
        let mut assigned = std::collections::HashSet::new();
        let mut base_names = BTreeMap::new();
        for operation in operations {
            let ident = self.get_method_name(operation).to_string();
            let plain = ident.strip_prefix("r#").unwrap_or(&ident);
            let name = if inherent.contains(plain) || !assigned.insert(plain.to_string()) {
                Self::allocate_name(&ident, &mut used)
            } else {
                ident
            };
            base_names.insert(operation.operation_id.as_str(), Self::to_field_ident(&name));
        }
        let mut plans = Vec::new();
        for &operation in operations {
            let method_ident = base_names[operation.operation_id.as_str()].clone();
            let success = self.get_success_response(analysis, operation);
            plans.push(self.make_client_method_plan(
                operation,
                method_ident.clone(),
                success.clone(),
                "buffered",
                None,
                false,
            ));
            if !self.multipart_binary_fields(operation, analysis).is_empty() {
                let base = format!("{method_ident}_with_multipart_filenames");
                let name = Self::allocate_name(&base, &mut used);
                let mut arguments: std::collections::HashSet<String> = self
                    .allocated_operation_params(operation)
                    .iter()
                    .map(|p| p.ident.to_string())
                    .collect();
                arguments.extend(["request".to_string(), "body".to_string()]);
                let filename_name = Self::allocate_name("multipart_filenames", &mut arguments);
                plans.push(self.make_client_method_plan(
                    operation,
                    Self::to_field_ident(&name),
                    success,
                    "buffered",
                    Some(Self::to_field_ident(&filename_name)),
                    false,
                ));
            }
            let Some(responses) = analysis.operation_responses.get(&operation.operation_id) else {
                continue;
            };
            let mut representations: BTreeMap<&str, Vec<(&str, &OperationRepresentation)>> =
                BTreeMap::new();
            for (status, response) in responses
                .iter()
                .filter(|(status, _)| status.starts_with('2'))
            {
                for (media_type, representation) in &response.representations {
                    representations
                        .entry(media_type)
                        .or_default()
                        .push((status, representation));
                }
            }
            let mut kind_counts = BTreeMap::new();
            for alternatives in representations.values() {
                let kind = Self::representation_kind(alternatives[0].1);
                *kind_counts.entry(kind).or_insert(0usize) += 1;
            }
            for (media_type, alternatives) in representations {
                let representation = alternatives[0].1;
                let incompatible = alternatives
                    .iter()
                    .any(|(_, other)| *other != representation);
                let groups: Vec<Vec<(&str, &OperationRepresentation)>> = if incompatible {
                    alternatives
                        .into_iter()
                        .map(|alternative| vec![alternative])
                        .collect()
                } else {
                    vec![alternatives]
                };
                for alternatives in groups {
                    let representation = alternatives[0].1;
                    let kind = Self::representation_kind(representation);
                    let suffix = if kind_counts[kind] > 1 {
                        format!("{kind}_{}", media_type.to_snake_case())
                    } else {
                        kind.to_string()
                    };
                    let suffix = if incompatible {
                        format!("{suffix}_status_{}", alternatives[0].0.to_ascii_lowercase())
                    } else {
                        suffix
                    };
                    let body = match representation {
                        OperationRepresentation::Json { schema_name } => {
                            ClientSuccessBody::Json(schema_name)
                        }
                        OperationRepresentation::Text => ClientSuccessBody::Text,
                        OperationRepresentation::Binary { .. } => ClientSuccessBody::Binary,
                        OperationRepresentation::EventStream => ClientSuccessBody::EventStream,
                    };
                    let selection = ClientSuccessSelection {
                        statuses: alternatives.iter().map(|(status, _)| *status).collect(),
                        excluded_statuses: responses
                            .keys()
                            .filter(|status| {
                                status.len() == 3
                                    && status.bytes().all(|c| c.is_ascii_digit())
                                    && !alternatives
                                        .iter()
                                        .any(|(selected, _)| selected == &status.as_str())
                            })
                            .map(String::as_str)
                            .collect(),
                        body,
                        accept: Some(media_type),
                    };
                    let default = self.get_success_response(analysis, operation);
                    let equivalent =
                        Self::success_bodies_are_compatible(default.body, selection.body)
                            && default.statuses == selection.statuses
                            && default.accept == selection.accept;
                    if !matches!(representation, OperationRepresentation::EventStream)
                        && !equivalent
                    {
                        let name =
                            Self::allocate_name(&format!("{method_ident}_{suffix}"), &mut used);
                        plans.push(self.make_client_method_plan(
                            operation,
                            Self::to_field_ident(&name),
                            selection.clone(),
                            "buffered",
                            None,
                            true,
                        ));
                    }
                    if matches!(representation, OperationRepresentation::Binary { .. }) {
                        let name = Self::allocate_name(
                            &format!("{method_ident}_{suffix}_stream"),
                            &mut used,
                        );
                        let live_selection = ClientSuccessSelection {
                            body: ClientSuccessBody::EventStream,
                            ..selection.clone()
                        };
                        let mut plan = self.make_client_method_plan(
                            operation,
                            Self::to_field_ident(&name),
                            live_selection,
                            "binary_stream",
                            None,
                            true,
                        );
                        plan.response.kind = "binary".to_string();
                        plans.push(plan);
                    }
                    if matches!(representation, OperationRepresentation::EventStream) {
                        if self.config().enable_sse_client {
                            let name =
                                Self::allocate_name(&format!("{method_ident}_sse"), &mut used);
                            let selection = ClientSuccessSelection {
                                body: ClientSuccessBody::ParsedSse,
                                ..selection
                            };
                            plans.push(self.make_client_method_plan(
                                operation,
                                Self::to_field_ident(&name),
                                selection,
                                "parsed_sse",
                                None,
                                true,
                            ));
                        } else if !equivalent {
                            let name =
                                Self::allocate_name(&format!("{method_ident}_{suffix}"), &mut used);
                            plans.push(self.make_client_method_plan(
                                operation,
                                Self::to_field_ident(&name),
                                selection,
                                "raw_event_stream",
                                None,
                                true,
                            ));
                        }
                    }
                }
            }
        }
        let response_count = plans.len();
        for index in 0..response_count {
            let plan = &plans[index];
            if !plan.validate_content_type
                || self
                    .multipart_binary_fields(plan.operation, analysis)
                    .is_empty()
            {
                continue;
            }
            let name = Self::allocate_name(
                &format!("{}_with_multipart_filenames", plan.method_ident),
                &mut used,
            );
            let mut arguments: std::collections::HashSet<String> = self
                .allocated_operation_params(plan.operation)
                .iter()
                .map(|p| p.ident.to_string())
                .collect();
            arguments.extend(["request".to_string(), "body".to_string()]);
            let argument = Self::allocate_name("multipart_filenames", &mut arguments);
            let mut filename_plan = self.make_client_method_plan(
                plan.operation,
                Self::to_field_ident(&name),
                plan.success.clone(),
                &plan.response.consumption,
                Some(Self::to_field_ident(&argument)),
                true,
            );
            filename_plan.response.kind = plan.response.kind.clone();
            plans.push(filename_plan);
        }
        let mut filename_plans: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
        for (index, plan) in plans.iter().enumerate() {
            if plan.multipart_filenames.is_some() {
                filename_plans
                    .entry(plan.operation.operation_id.as_str())
                    .or_default()
                    .push(index);
            }
        }
        for index in 0..plans.len() {
            if plans[index].multipart_filenames.is_some() {
                continue;
            }
            let target = filename_plans
                .get(plans[index].operation.operation_id.as_str())
                .into_iter()
                .flatten()
                .map(|candidate| &plans[*candidate])
                .find(|candidate| {
                    candidate.response.statuses == plans[index].response.statuses
                        && candidate.response.media_type == plans[index].response.media_type
                        && candidate.response.consumption == plans[index].response.consumption
                })
                .map(|candidate| candidate.method_ident.clone());
            plans[index].filenames_method_ident = target;
        }
        self.plan_operation_builders(analysis, &mut plans);
        plans
    }

    fn representation_kind(representation: &OperationRepresentation) -> &'static str {
        match representation {
            OperationRepresentation::Json { .. } => "json",
            OperationRepresentation::Text => "text",
            OperationRepresentation::Binary { .. } => "binary",
            OperationRepresentation::EventStream => "event_stream",
        }
    }

    fn make_client_method_plan<'a>(
        &self,
        operation: &'a OperationInfo,
        method_ident: syn::Ident,
        success: ClientSuccessSelection<'a>,
        consumption: &str,
        multipart_filenames: Option<syn::Ident>,
        validate_content_type: bool,
    ) -> ClientMethodPlan<'a> {
        let precise_stream = matches!(success.body, ClientSuccessBody::EventStream)
            && (validate_content_type || multipart_filenames.is_some());
        let mut captures = Vec::new();
        let request = if precise_stream {
            self.generate_request_param_with_capture(operation, Some(&mut captures))
        } else {
            self.generate_request_param(operation)
        };
        let generics = if captures.is_empty() {
            TokenStream::new()
        } else {
            quote! { <#(#captures: AsRef<str>),*> }
        };
        let request_parameters = if let Some(ident) = &multipart_filenames {
            if request.is_empty() {
                quote! { #ident: &[(&str, &str)] }
            } else {
                quote! { #request, #ident: &[(&str, &str)] }
            }
        } else {
            request
        };
        let response_type = if precise_stream {
            quote! { impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> + use<#(#captures),*> }
        } else if consumption == "parsed_sse" {
            quote! { super::sse::BoxSseStream<Result<super::sse::SseEvent<String>, super::sse::StreamingError>> }
        } else {
            self.response_type_for_body(success.body)
        };
        let kind = match success.body {
            ClientSuccessBody::Json(_) => "json",
            ClientSuccessBody::Text => "text",
            ClientSuccessBody::Binary => "binary",
            ClientSuccessBody::EventStream | ClientSuccessBody::ParsedSse => "event_stream",
            ClientSuccessBody::Empty => "empty",
        };
        let consumption = if matches!(success.body, ClientSuccessBody::EventStream)
            && consumption == "buffered"
        {
            "raw_event_stream"
        } else {
            consumption
        };
        ClientMethodPlan {
            operation,
            method_ident,
            request_parameters,
            generics,
            response_type,
            error_type: self.op_error_type_token(operation),
            response: ClientResponsePlan {
                statuses: success.statuses.iter().map(|s| (*s).to_string()).collect(),
                excluded_statuses: success
                    .excluded_statuses
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect(),
                media_type: success.accept.map(str::to_owned),
                kind: kind.to_string(),
                consumption: consumption.to_string(),
            },
            multipart_filenames,
            filenames_method_ident: None,
            builder_method_ident: None,
            builder_type_ident: None,
            success,
            validate_content_type,
        }
    }

    fn plan_operation_builders(
        &self,
        analysis: &SchemaAnalysis,
        plans: &mut [ClientMethodPlan<'_>],
    ) {
        if !self.config().builders.enabled {
            return;
        }

        let mut used_entry_methods: std::collections::HashSet<String> = plans
            .iter()
            .map(|plan| plan.method_ident.to_string())
            .collect();
        let mut used_type_names = std::collections::HashSet::new();
        for schema_name in analysis.schemas.keys() {
            let rust_name = self.to_rust_type_name(schema_name);
            used_type_names.insert(rust_name.clone());
            // Request-model builders live in `types.rs` and are imported by
            // glob into the client module. Reserve their conventional names
            // so operation builders cannot create ambiguous re-exports.
            used_type_names.insert(format!("{rust_name}Builder"));
        }
        used_type_names.insert("HttpClient".to_string());
        used_type_names.insert("ApiOpError".to_string());
        used_type_names.extend(
            [
                "ClientBuilder",
                "ClientWithMiddleware",
                "RetryConfig",
                "HttpError",
                "BTreeMap",
            ]
            .into_iter()
            .map(str::to_string),
        );
        for plan in plans.iter() {
            let operation = plan.operation;
            used_type_names.insert(self.op_error_enum_ident(operation).to_string());
            used_type_names.extend(
                operation
                    .parameters
                    .iter()
                    .filter(|parameter| parameter.enum_values.is_some())
                    .map(|parameter| parameter.rust_type.clone()),
            );
        }

        for plan in plans.iter_mut() {
            let operation = plan.operation;
            let allocated_params = self.allocated_operation_params(operation);
            let body_plan = self.body_model_plan(operation, analysis);
            let optional_param_count = allocated_params
                .iter()
                .filter(|allocated| !Self::builder_param_is_required(allocated.param))
                .count();
            let optional_body_count =
                usize::from(operation.request_body.is_some() && !operation.request_body_required);
            let body_field_count = body_plan
                .as_ref()
                .filter(|plan| {
                    operation.request_body_required
                        || matches!(
                            &plan.required_construction,
                            RequiredBodyConstruction::Default
                        )
                })
                .map_or(0, |plan| plan.optional_fields.len());
            let optional_count = optional_param_count + optional_body_count + body_field_count;
            if optional_count <= self.config().builders.threshold {
                continue;
            }

            let flat_method = plan.method_ident.clone();
            let entry_base = format!("{flat_method}_builder");
            let entry_name = Self::allocate_name(&entry_base, &mut used_entry_methods);
            let entry_ident = Self::to_field_ident(&entry_name);

            let builder_base = format!("{}Builder", flat_method.to_string().to_pascal_case());
            let builder_name = Self::allocate_type_name(&builder_base, &mut used_type_names);
            let builder_ident = format_ident!("{builder_name}");

            plan.builder_method_ident = Some(entry_ident);
            plan.builder_type_ident = Some(builder_ident);
        }
    }

    fn generate_operation_builders(
        &self,
        analysis: &SchemaAnalysis,
        plans: &[ClientMethodPlan<'_>],
    ) -> (Vec<TokenStream>, Vec<TokenStream>) {
        let mut definitions = Vec::new();
        let mut entries = Vec::new();
        for plan in plans {
            let (Some(entry_ident), Some(builder_ident)) =
                (&plan.builder_method_ident, &plan.builder_type_ident)
            else {
                continue;
            };
            let operation = plan.operation;
            let allocated_params = self.allocated_operation_params(operation);
            let (definition, entry) = self.generate_single_operation_builder(
                analysis,
                operation,
                &allocated_params,
                self.body_model_plan(operation, analysis),
                &plan.method_ident,
                entry_ident,
                builder_ident,
                plan,
            );
            definitions.push(definition);
            entries.push(entry);
        }
        (definitions, entries)
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_single_operation_builder(
        &self,
        _analysis: &SchemaAnalysis,
        operation: &OperationInfo,
        allocated_params: &[AllocatedOperationParam<'_>],
        body_plan: Option<BodyModelPlan>,
        flat_method: &syn::Ident,
        entry_ident: &syn::Ident,
        builder_ident: &syn::Ident,
        plan: &ClientMethodPlan<'_>,
    ) -> (TokenStream, TokenStream) {
        let mut fields = vec![quote! { client: &'a HttpClient }];
        let mut entry_parameters = Vec::new();
        let mut initializers = vec![quote! { client: self }];
        let mut call_arguments = Vec::new();
        let mut setters = Vec::new();
        let mut used_entry_params = std::collections::HashSet::new();
        let mut used_methods = std::collections::HashSet::from(["send".to_string()]);

        for allocated in allocated_params {
            let field_ident = &allocated.ident;
            let storage_type = self.builder_param_storage_type(allocated.param);
            if Self::builder_param_is_required(allocated.param) {
                fields.push(quote! { #field_ident: #storage_type });
                let entry_name =
                    Self::allocate_name(&field_ident.to_string(), &mut used_entry_params);
                let entry_param = Self::to_field_ident(&entry_name);
                if Self::param_has_impl_as_ref_type(allocated.param) {
                    entry_parameters.push(quote! { #entry_param: impl Into<String> });
                    initializers.push(quote! { #field_ident: #entry_param.into() });
                } else {
                    entry_parameters.push(quote! { #entry_param: #storage_type });
                    initializers.push(quote! { #field_ident: #entry_param });
                }
            } else {
                fields.push(quote! { #field_ident: Option<#storage_type> });
                initializers.push(quote! { #field_ident: None });
                let setter_ident =
                    Self::allocate_builder_method(&field_ident.to_string(), &mut used_methods);
                let wire_name = &allocated.param.name;
                let assignment = if Self::param_has_impl_as_ref_type(allocated.param) {
                    quote! { self.#field_ident = Some(#field_ident.into()); }
                } else {
                    quote! { self.#field_ident = Some(#field_ident); }
                };
                let setter_type = if Self::param_has_impl_as_ref_type(allocated.param) {
                    quote! { impl Into<String> }
                } else {
                    storage_type.clone()
                };
                setters.push(quote! {
                    #[doc = concat!("Set the optional `", #wire_name, "` operation parameter.")]
                    #[must_use]
                    pub fn #setter_ident(mut self, #field_ident: #setter_type) -> Self {
                        #assignment
                        self
                    }
                });
            }
            call_arguments.push(quote! { self.#field_ident });
        }

        if let Some(body_plan) = body_plan {
            let BodyModelPlan {
                body_ident,
                body_type,
                required_construction,
                optional_fields,
            } = body_plan;
            let can_initialize_optional_body =
                matches!(&required_construction, RequiredBodyConstruction::Default);
            if operation.request_body_required {
                fields.push(quote! { #body_ident: #body_type });
                match required_construction {
                    RequiredBodyConstruction::Default => {
                        initializers.push(quote! { #body_ident: Default::default() });
                    }
                    RequiredBodyConstruction::New(constructor_params) => {
                        let mut constructor_args = Vec::new();
                        for constructor in constructor_params {
                            let preferred = constructor.preferred_ident.to_string();
                            let entry_name =
                                Self::allocate_name(&preferred, &mut used_entry_params);
                            let entry_param = Self::to_field_ident(&entry_name);
                            let value_type = constructor.value_type;
                            entry_parameters.push(quote! { #entry_param: #value_type });
                            constructor_args.push(entry_param);
                        }
                        initializers.push(quote! {
                            #body_ident: #body_type::new(#(#constructor_args),*)
                        });
                    }
                    RequiredBodyConstruction::Whole => {
                        let entry_name =
                            Self::allocate_name(&body_ident.to_string(), &mut used_entry_params);
                        let entry_param = Self::to_field_ident(&entry_name);
                        entry_parameters.push(quote! { #entry_param: #body_type });
                        initializers.push(quote! { #body_ident: #entry_param });
                    }
                }
            } else {
                fields.push(quote! { #body_ident: Option<#body_type> });
                initializers.push(quote! { #body_ident: None });
            }

            let body_setter =
                Self::allocate_builder_method(&body_ident.to_string(), &mut used_methods);
            let body_assignment = if operation.request_body_required {
                quote! { self.#body_ident = #body_ident; }
            } else {
                quote! { self.#body_ident = Some(#body_ident); }
            };
            setters.push(quote! {
                /// Replace the complete request body.
                #[must_use]
                pub fn #body_setter(mut self, #body_ident: #body_type) -> Self {
                    #body_assignment
                    self
                }
            });

            if operation.request_body_required || can_initialize_optional_body {
                for field in optional_fields {
                    let setter_ident = Self::allocate_builder_method(
                        &field.preferred_method_name,
                        &mut used_methods,
                    );
                    let preferred_method_name = field.preferred_method_name.clone();
                    let value_ident = field.value_ident;
                    let value_type = field.value_type;
                    let wire_name = field.wire_name;
                    let access_path = field.access_path;
                    let assignment = if operation.request_body_required {
                        let mut target = quote! { self.#body_ident };
                        for access in &access_path {
                            target = quote! { #target.#access };
                        }
                        if field.tri_state {
                            quote! { #target = Some(Some(#value_ident)); }
                        } else {
                            quote! { #target = Some(#value_ident); }
                        }
                    } else {
                        let mut target = quote! { request };
                        for access in &access_path {
                            target = quote! { #target.#access };
                        }
                        if field.tri_state {
                            quote! {
                                let request = self.#body_ident.get_or_insert_with(Default::default);
                                #target = Some(Some(#value_ident));
                            }
                        } else {
                            quote! {
                                let request = self.#body_ident.get_or_insert_with(Default::default);
                                #target = Some(#value_ident);
                            }
                        }
                    };
                    setters.push(quote! {
                        #[doc = concat!("Set the optional request-body field `", #wire_name, "`.")]
                        #[must_use]
                        pub fn #setter_ident(mut self, #value_ident: #value_type) -> Self {
                            #assignment
                            self
                        }
                    });
                    if field.tri_state {
                        let null_ident = Self::allocate_builder_method(
                            &format!("{preferred_method_name}_null"),
                            &mut used_methods,
                        );
                        let absent_ident = Self::allocate_builder_method(
                            &format!("{preferred_method_name}_absent"),
                            &mut used_methods,
                        );
                        let null_assignment = if operation.request_body_required {
                            let mut target = quote! { self.#body_ident };
                            for access in &access_path {
                                target = quote! { #target.#access };
                            }
                            quote! { #target = Some(None); }
                        } else {
                            let mut target = quote! { request };
                            for access in &access_path {
                                target = quote! { #target.#access };
                            }
                            quote! {
                                let request = self.#body_ident.get_or_insert_with(Default::default);
                                #target = Some(None);
                            }
                        };
                        let absent_assignment = if operation.request_body_required {
                            let mut target = quote! { self.#body_ident };
                            for access in &access_path {
                                target = quote! { #target.#access };
                            }
                            quote! { #target = None; }
                        } else {
                            let mut target = quote! { request };
                            for access in &access_path {
                                target = quote! { #target.#access };
                            }
                            quote! {
                                if let Some(request) = self.#body_ident.as_mut() {
                                    #target = None;
                                }
                            }
                        };
                        setters.push(quote! {
                            #[doc = concat!("Set the optional nullable request-body field `", #wire_name, "` to JSON null.")]
                            #[must_use]
                            pub fn #null_ident(mut self) -> Self {
                                #null_assignment
                                self
                            }

                            #[doc = concat!("Omit the optional nullable request-body field `", #wire_name, "`.")]
                            #[must_use]
                            pub fn #absent_ident(mut self) -> Self {
                                #absent_assignment
                                self
                            }
                        });
                    }
                }
            }
            call_arguments.push(quote! { self.#body_ident });
        }

        let mut filename_field = None;
        if plan.multipart_filenames.is_some() || plan.filenames_method_ident.is_some() {
            let mut used_fields: std::collections::HashSet<String> = allocated_params
                .iter()
                .map(|p| p.ident.to_string())
                .collect();
            used_fields.extend([
                "request".to_string(),
                "body".to_string(),
                "client".to_string(),
            ]);
            let field = Self::to_field_ident(&Self::allocate_name(
                "multipart_filenames",
                &mut used_fields,
            ));
            fields.push(quote! { #field: Vec<(String, String)> });
            initializers.push(quote! { #field: Vec::new() });
            let setter = Self::allocate_builder_method("multipart_filenames", &mut used_methods);
            setters.push(quote! {
                /// Set request-local filenames by declared binary field wire name.
                #[must_use]
                pub fn #setter(mut self, filenames: &[(&str, &str)]) -> Self {
                    self.#field = filenames.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect();
                    self
                }
            });
            if plan.multipart_filenames.is_some() {
                call_arguments.push(quote! { &self.#field.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect::<Vec<_>>() });
            }
            filename_field = Some(field);
        }
        let response_type = self.response_type_for_body(plan.success.body);
        let error_type = &plan.error_type;
        let operation_id = &operation.operation_id;
        let send = if let (Some(field), Some(method)) =
            (&filename_field, &plan.filenames_method_ident)
        {
            if matches!(plan.success.body, ClientSuccessBody::EventStream) {
                quote! {
                    if self.#field.is_empty() { self.client.#flat_method(#(#call_arguments),*).await.map(futures_util::future::Either::Left) }
                    else {
                        let filenames: Vec<_> = self.#field.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
                        self.client.#method(#(#call_arguments,)* &filenames).await.map(futures_util::future::Either::Right)
                    }
                }
            } else {
                quote! {
                    if self.#field.is_empty() { self.client.#flat_method(#(#call_arguments),*).await }
                    else {
                        let filenames: Vec<_> = self.#field.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
                        self.client.#method(#(#call_arguments,)* &filenames).await
                    }
                }
            }
        } else {
            quote! { self.client.#flat_method(#(#call_arguments),*).await }
        };
        let definition = quote! {
            #[doc = concat!("Additive request builder for `", #operation_id, "`.")]
            #[must_use]
            pub struct #builder_ident<'a> {
                #(#fields,)*
            }

            impl<'a> #builder_ident<'a> {
                #(#setters)*

                /// Send the request through the existing flat operation method.
                pub async fn send(self) -> Result<#response_type, ApiOpError<#error_type>> {
                    #send
                }
            }
        };
        let entry = quote! {
            #[doc = concat!("Start an additive builder for `", #operation_id, "`.")]
            pub fn #entry_ident(
                &self,
                #(#entry_parameters),*
            ) -> #builder_ident<'_> {
                #builder_ident {
                    #(#initializers,)*
                }
            }
        };
        (definition, entry)
    }

    fn allocate_name(base: &str, used: &mut std::collections::HashSet<String>) -> String {
        let raw = base.starts_with("r#");
        let base = base.strip_prefix("r#").unwrap_or(base);
        let mut candidate = base.to_string();
        let mut suffix = 2;
        while used.contains(&candidate) || used.contains(&format!("r#{candidate}")) {
            candidate = format!("{base}_{suffix}");
            suffix += 1;
        }
        used.insert(candidate.clone());
        if raw {
            format!("r#{candidate}")
        } else {
            candidate
        }
    }

    fn allocate_type_name(base: &str, used: &mut std::collections::HashSet<String>) -> String {
        if used.insert(base.to_string()) {
            return base.to_string();
        }

        let mut suffix = 2;
        loop {
            let candidate = format!("{base}{suffix}");
            if used.insert(candidate.clone()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn allocate_builder_method(
        preferred: &str,
        used: &mut std::collections::HashSet<String>,
    ) -> syn::Ident {
        let plain = preferred.strip_prefix("r#").unwrap_or(preferred);
        let base = if used.contains(preferred) {
            format!("with_{plain}")
        } else {
            preferred.to_string()
        };
        let allocated = Self::allocate_name(&base, used);
        Self::to_field_ident(&allocated)
    }

    fn allocated_operation_params<'a>(
        &self,
        operation: &'a OperationInfo,
    ) -> Vec<AllocatedOperationParam<'a>> {
        // Analyzer resolves parameter collisions once. Every request path and
        // flat/builder signature uses those identifiers.
        let mut used: std::collections::HashSet<String> =
            ["request", "body", "req", "request_url", "client", "form"]
                .into_iter()
                .map(str::to_owned)
                .collect();
        let mut allocated = Vec::new();
        for location in ["path", "query", "header", "cookie"] {
            for parameter in &operation.parameters {
                if parameter.location != location {
                    continue;
                }
                let raw = self.param_ident_str(parameter);
                let chosen = Self::allocate_name(&raw, &mut used);
                allocated.push(AllocatedOperationParam {
                    param: parameter,
                    ident: Self::to_field_ident(&chosen),
                });
            }
        }
        allocated
    }

    fn operation_param_ident(
        &self,
        operation: &OperationInfo,
        parameter: &ParameterInfo,
    ) -> String {
        self.allocated_operation_params(operation)
            .into_iter()
            .find(|allocated| std::ptr::eq(allocated.param, parameter))
            .map(|allocated| allocated.ident.to_string())
            .unwrap_or_else(|| self.param_ident_str(parameter))
    }

    fn builder_param_is_required(parameter: &ParameterInfo) -> bool {
        // The existing flat signature always emits path parameters as bare
        // values. Invalid real-world specs sometimes omit `required: true`;
        // mirror the flat contract so builder delegation remains type-correct.
        parameter.location == "path" || parameter.required
    }

    fn builder_param_storage_type(&self, parameter: &ParameterInfo) -> TokenStream {
        self.get_param_owned_rust_type(parameter)
    }

    fn param_has_impl_as_ref_type(parameter: &ParameterInfo) -> bool {
        !matches!(
            &parameter.query_serialization,
            Some(
                crate::analysis::QuerySerialization::FormExplodedArray { .. }
                    | crate::analysis::QuerySerialization::FormArray { .. }
                    | crate::analysis::QuerySerialization::SimpleHeaderArray { .. },
            )
        ) && Self::param_uses_as_ref_str(parameter)
    }

    fn body_model_plan(
        &self,
        operation: &OperationInfo,
        analysis: &SchemaAnalysis,
    ) -> Option<BodyModelPlan> {
        use crate::analysis::{RequestBodyContent, SchemaType};

        let request_body = operation.request_body.as_ref()?;
        let (body_name, body_ident) = match request_body {
            RequestBodyContent::Json { schema_name, .. }
            | RequestBodyContent::FormUrlEncoded { schema_name, .. }
            | RequestBodyContent::Multipart { schema_name, .. } => {
                (schema_name.as_str(), format_ident!("request"))
            }
            RequestBodyContent::OctetStream { .. }
            | RequestBodyContent::Binary { .. }
            | RequestBodyContent::Unsupported { .. } => {
                return Some(BodyModelPlan {
                    body_ident: format_ident!("body"),
                    body_type: quote! { Vec<u8> },
                    required_construction: RequiredBodyConstruction::Whole,
                    optional_fields: Vec::new(),
                });
            }
            RequestBodyContent::TextPlain { .. } => {
                return Some(BodyModelPlan {
                    body_ident: format_ident!("body"),
                    body_type: quote! { String },
                    required_construction: RequiredBodyConstruction::Whole,
                    optional_fields: Vec::new(),
                });
            }
            RequestBodyContent::SchemaLess { .. } => return None,
        };
        let body_type_name = self.to_rust_type_name(body_name);
        let body_type = syn::Ident::new(&body_type_name, proc_macro2::Span::call_site());
        let Some((resolved_name, resolved_schema)) =
            self.resolve_reference_schema(body_name, analysis)
        else {
            return Some(BodyModelPlan {
                body_ident,
                body_type: quote! { #body_type },
                required_construction: RequiredBodyConstruction::Whole,
                optional_fields: Vec::new(),
            });
        };

        let mut optional_fields = Vec::new();
        let mut stack = std::collections::HashSet::new();
        self.collect_optional_body_fields(
            resolved_name,
            Vec::new(),
            analysis,
            &mut stack,
            &mut optional_fields,
        );

        let required_construction = match &resolved_schema.schema_type {
            SchemaType::Object {
                properties,
                required,
                additional_properties,
                ..
            } if !self.is_discriminated_variant(resolved_name, analysis) => {
                let emitted = self.emitted_object_properties(
                    resolved_name,
                    properties,
                    required,
                    additional_properties,
                    analysis,
                );
                let required_fields: Vec<_> = emitted
                    .iter()
                    .filter(|field| field.is_required)
                    .map(|field| BodyConstructorParam {
                        preferred_ident: field.ident.clone(),
                        value_type: field.field_type.clone(),
                    })
                    .collect();
                if required_fields.is_empty() {
                    RequiredBodyConstruction::Default
                } else if emitted.iter().any(|field| !field.is_required)
                    || additional_properties.is_open()
                {
                    RequiredBodyConstruction::New(required_fields)
                } else {
                    RequiredBodyConstruction::Whole
                }
            }
            _ => RequiredBodyConstruction::Whole,
        };

        Some(BodyModelPlan {
            body_ident,
            body_type: quote! { #body_type },
            required_construction,
            optional_fields,
        })
    }

    fn resolve_reference_schema<'a>(
        &self,
        schema_name: &'a str,
        analysis: &'a SchemaAnalysis,
    ) -> Option<(&'a str, &'a crate::analysis::AnalyzedSchema)> {
        let mut current = schema_name;
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert(current) {
                return None;
            }
            let schema = analysis.schemas.get(current)?;
            if let crate::analysis::SchemaType::Reference { target } = &schema.schema_type {
                current = target;
            } else {
                return Some((current, schema));
            }
        }
    }

    fn collect_optional_body_fields(
        &self,
        schema_name: &str,
        access_path: Vec<syn::Ident>,
        analysis: &SchemaAnalysis,
        stack: &mut std::collections::HashSet<String>,
        output: &mut Vec<BodyFieldPlan>,
    ) {
        use crate::analysis::SchemaType;
        if !stack.insert(schema_name.to_string()) {
            return;
        }
        let Some(schema) = analysis.schemas.get(schema_name) else {
            stack.remove(schema_name);
            return;
        };
        match &schema.schema_type {
            SchemaType::Reference { target } => {
                self.collect_optional_body_fields(target, access_path, analysis, stack, output);
            }
            SchemaType::Object {
                properties,
                required,
                additional_properties,
                ..
            } if !self.is_discriminated_variant(schema_name, analysis) => {
                for field in self.emitted_object_properties(
                    schema_name,
                    properties,
                    required,
                    additional_properties,
                    analysis,
                ) {
                    if field.is_required {
                        continue;
                    }
                    let mut field_path = access_path.clone();
                    field_path.push(field.ident.clone());
                    output.push(BodyFieldPlan {
                        wire_name: field.wire_name.to_string(),
                        preferred_method_name: field.ident.to_string(),
                        value_ident: field.ident.clone(),
                        value_type: self.generate_property_base_type(
                            schema_name,
                            field.wire_name,
                            field.property,
                            analysis,
                        ),
                        access_path: field_path,
                        tri_state: self.property_is_tri_state(
                            schema_name,
                            field.wire_name,
                            field.property,
                            field.is_required,
                        ),
                    });
                }
            }
            SchemaType::Composition { schemas } => {
                for (index, schema_ref) in schemas.iter().enumerate() {
                    let mut nested_path = access_path.clone();
                    nested_path.push(format_ident!("part_{index}"));
                    self.collect_optional_body_fields(
                        &schema_ref.target,
                        nested_path,
                        analysis,
                        stack,
                        output,
                    );
                }
            }
            _ => {}
        }
        stack.remove(schema_name);
    }

    fn is_discriminated_variant(&self, schema_name: &str, analysis: &SchemaAnalysis) -> bool {
        analysis.schemas.values().any(|schema| {
            matches!(
                &schema.schema_type,
                crate::analysis::SchemaType::DiscriminatedUnion { variants, .. }
                    if variants.iter().any(|variant| variant.type_name == schema_name)
            )
        })
    }

    /// Emit inline enum types for parameters whose schema is `type: string`
    /// with `enum` or `const`. The generated enum implements `Display` so it
    /// drops into the existing `format!`-based path/query templating without
    /// any special-casing at the call site. See issue #10 follow-up.
    fn generate_param_enum_types(&self, operations: &[&OperationInfo]) -> TokenStream {
        let mut by_name: BTreeMap<String, &ParameterInfo> = BTreeMap::new();
        for op in operations {
            for param in &op.parameters {
                if param.enum_values.is_some() {
                    by_name.entry(param.rust_type.clone()).or_insert(param);
                }
            }
        }

        if by_name.is_empty() {
            return quote! {};
        }

        let defs: Vec<TokenStream> = by_name
            .values()
            .map(|param| self.generate_single_param_enum(param))
            .collect();

        quote! { #(#defs)* }
    }

    fn generate_single_param_enum(&self, param: &ParameterInfo) -> TokenStream {
        let Some(values) = param.enum_values.as_deref() else {
            return quote! {};
        };

        let enum_ident = format_ident!("{}", param.rust_type);

        // Dedupe variant names. Real-world specs use sort enums like
        // `["created_at", "-created_at"]` (descending prefix), and both
        // PascalCase to `CreatedAt`. Suffix collisions with `_2`/`_3`/…
        // while keeping each `serde(rename)` pointing at the original
        // wire string.
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        // `x-enum-varnames` wins over the naming heuristic when the spec
        // supplies it — the whole point of the extension is that the author
        // knows better than a transformation of the wire string. Schema-level
        // enums already honored it; parameter enums did not, so the same spec
        // produced different variant names depending on where its enum lived.
        // Suffix disambiguation still applies, since nothing stops a spec from
        // declaring two names that collide once converted to an identifier.
        let variant_names: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let base = param
                    .enum_varnames
                    .as_ref()
                    .and_then(|names| names.get(index))
                    .map(|name| self.to_rust_enum_variant(name))
                    .unwrap_or_else(|| self.to_rust_enum_variant(value));
                let mut chosen = base.clone();
                let mut suffix = 2;
                while !used.insert(chosen.clone()) {
                    chosen = format!("{base}_{suffix}");
                    suffix += 1;
                }
                chosen
            })
            .collect();

        let variants: Vec<TokenStream> = values
            .iter()
            .zip(&variant_names)
            .map(|(value, name)| {
                let variant_ident = format_ident!("{}", name);
                quote! {
                    #[serde(rename = #value)]
                    #variant_ident,
                }
            })
            .collect();

        let display_arms: Vec<TokenStream> = values
            .iter()
            .zip(&variant_names)
            .map(|(value, name)| {
                let variant_ident = format_ident!("{}", name);
                quote! { Self::#variant_ident => #value, }
            })
            .collect();

        let doc = format!(
            "Allowed values for the `{}` {} parameter.",
            param.name, param.location
        );

        quote! {
            #[doc = #doc]
            #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
            pub enum #enum_ident {
                #(#variants)*
            }

            impl #enum_ident {
                pub fn as_str(&self) -> &'static str {
                    match self {
                        #(#display_arms)*
                    }
                }
            }

            impl std::fmt::Display for #enum_ident {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(self.as_str())
                }
            }

            impl AsRef<str> for #enum_ident {
                fn as_ref(&self) -> &str {
                    self.as_str()
                }
            }
        }
    }

    /// Generate the per-operation typed error enum, if the op has any non-2xx
    /// responses with a body schema. Returns None when the op has no declared
    /// error bodies — those operations use `ApiOpError<serde_json::Value>` so
    /// the raw response body is still inspectable.
    fn generate_op_error_enum(&self, op: &OperationInfo) -> Option<TokenStream> {
        let variants: Vec<(String, String)> = op
            .response_schemas
            .iter()
            .filter(|(code, _)| !code.starts_with('2'))
            .map(|(code, schema)| (code.clone(), schema.clone()))
            .collect();

        if variants.is_empty() {
            return None;
        }

        let enum_ident = self.op_error_enum_ident(op);
        let variant_decls: Vec<TokenStream> = variants
            .iter()
            .map(|(code, schema)| {
                let variant_ident = Self::op_error_variant_ident(code);
                let payload_ty_name = self.to_rust_type_name(schema);
                let payload_ty = syn::Ident::new(&payload_ty_name, proc_macro2::Span::call_site());
                quote! { #variant_ident(#payload_ty) }
            })
            .collect();

        let doc = format!(
            "Typed error responses for `{}`. One variant per declared non-2xx response.",
            op.operation_id
        );

        Some(quote! {
            #[doc = #doc]
            #[derive(Debug, Clone)]
            pub enum #enum_ident {
                #(#variant_decls,)*
            }
        })
    }

    /// Type name (Ident) for the per-op error enum, e.g. `ListTodosApiError`.
    fn op_error_enum_ident(&self, op: &OperationInfo) -> syn::Ident {
        use heck::ToPascalCase;
        let name = format!(
            "{}ApiError",
            op.operation_id.replace('.', "_").to_pascal_case()
        );
        syn::Ident::new(&name, proc_macro2::Span::call_site())
    }

    /// Variant name for a status code: "400" → Status400, "default" → Default,
    /// "4XX" → Status4xx.
    fn op_error_variant_ident(status_code: &str) -> syn::Ident {
        let raw = match status_code {
            "default" | "Default" => "Default".to_string(),
            other if other.chars().all(|c| c.is_ascii_digit()) => format!("Status{other}"),
            other => format!("Status{}", other.to_ascii_lowercase()),
        };
        syn::Ident::new(&raw, proc_macro2::Span::call_site())
    }

    /// Token stream for the type plugged into `ApiOpError<T>` for an op:
    /// either the per-op enum, or `serde_json::Value` for ops with no
    /// declared error body schemas.
    fn op_error_type_token(&self, op: &OperationInfo) -> TokenStream {
        if op
            .response_schemas
            .iter()
            .any(|(code, _)| !code.starts_with('2'))
        {
            let ident = self.op_error_enum_ident(op);
            quote! { #ident }
        } else {
            quote! { serde_json::Value }
        }
    }

    /// Generate a single operation method
    fn generate_planned_operation_method(
        &self,
        analysis: &SchemaAnalysis,
        plan: &ClientMethodPlan<'_>,
    ) -> TokenStream {
        let op = plan.operation;
        let method_name = &plan.method_ident;
        let http_method_call = self.http_method_call(op);
        let path = &op.path;
        let request_param = &plan.request_parameters;
        let generics = &plan.generics;
        let request_body = self.generate_request_body_with_filenames(
            op,
            analysis,
            plan.multipart_filenames.as_ref(),
        );
        let filename_validation = self.generate_multipart_filename_validation(
            op,
            analysis,
            plan.multipart_filenames.as_ref(),
        );
        let query_params = self.generate_query_params(op);
        let header_params = self.generate_header_params(op);
        let cookie_params = self.generate_cookie_params(op);
        let auth_application = self.generate_auth_application();
        let success = plan.success.clone();
        let response_type = &plan.response_type;
        let op_error_type = &plan.error_type;
        let accept = success.accept;
        let error_handling = self.generate_error_handling(op, success.clone());
        let content_validation = if plan.validate_content_type {
            self.generate_response_content_type_validation(op, &success)
        } else {
            TokenStream::new()
        };
        let (custom_headers, accept_header) = if let Some(media_type) = accept {
            (
                quote! {
                    for (name, value) in &self.custom_headers {
                        if !name.eq_ignore_ascii_case("accept") {
                            req = req.header(name, value);
                        }
                    }
                },
                quote! {
                    req = req.header(reqwest::header::ACCEPT, #media_type);
                },
            )
        } else {
            (
                quote! {
                    for (name, value) in &self.custom_headers {
                        req = req.header(name, value);
                    }
                },
                TokenStream::new(),
            )
        };
        let url_construction = self.generate_url_construction(path, op);
        let doc_comment = self.generate_operation_doc_comment(op);
        let variant_doc = if plan.validate_content_type {
            let description = format!(
                "Select `{}` as {}; accepts response statuses {}. Other successful statuses and mismatched Content-Type values are returned as inspectable API errors.",
                plan.response.media_type.as_deref().unwrap_or("no body"),
                plan.response.consumption,
                plan.response.statuses.join(", ")
            );
            quote! { #[doc = #description] }
        } else {
            TokenStream::new()
        };
        let filename_doc = if plan.multipart_filenames.is_some() {
            quote! { #[doc = "Override filenames by binary field wire name for this request. Unknown or duplicate keys are configuration errors; absent fields stay absent."] }
        } else {
            TokenStream::new()
        };
        let stream_doc = if plan.response.consumption == "binary_stream" {
            quote! { #[doc = "Success chunks are returned live without a total body-size limit. Error responses use the configured buffered body-size limit."] }
        } else {
            TokenStream::new()
        };

        quote! {
            #doc_comment
            #variant_doc
            #filename_doc
            #stream_doc
            pub async fn #method_name #generics(
                &self,
                #request_param
            ) -> Result<#response_type, ApiOpError<#op_error_type>> {
                #filename_validation
                #url_construction

                let mut req = #http_method_call;
                #request_body

                #query_params
                #header_params
                #cookie_params

                // Apply configured authentication (T3). Was previously
                // hardcoded to bearer_auth regardless of GeneratorConfig.
                #auth_application

                // Add custom headers
                #custom_headers

                // Keep content negotiation aligned with the generated return type,
                // replacing any custom Accept value for this operation.
                #accept_header

                let response = req.send().await?;
                #content_validation
                #error_handling
            }
        }
    }

    /// T3: emit the auth-token application based on the configured AuthConfig.
    /// Default (no config) is Bearer on Authorization. ApiKey emits a custom
    /// header. Custom honors header_value_prefix.
    fn generate_auth_application(&self) -> TokenStream {
        use crate::http_config::AuthConfig;
        match &self.config().auth_config {
            Some(AuthConfig::Bearer { header_name }) if header_name == "Authorization" => quote! {
                if let Some(api_key) = &self.api_key {
                    req = req.bearer_auth(api_key);
                }
            },
            Some(AuthConfig::Bearer { header_name }) => {
                let h = header_name.clone();
                quote! {
                    if let Some(api_key) = &self.api_key {
                        req = req.header(#h, format!("Bearer {}", api_key));
                    }
                }
            }
            Some(AuthConfig::ApiKey { header_name }) => {
                let h = header_name.clone();
                quote! {
                    if let Some(api_key) = &self.api_key {
                        req = req.header(#h, api_key.as_str());
                    }
                }
            }
            Some(AuthConfig::Custom {
                header_name,
                header_value_prefix,
            }) => {
                let h = header_name.clone();
                let prefix = header_value_prefix.clone().unwrap_or_default();
                if prefix.is_empty() {
                    quote! {
                        if let Some(api_key) = &self.api_key {
                            req = req.header(#h, api_key.as_str());
                        }
                    }
                } else {
                    let format_str = format!("{}{{}}", prefix);
                    quote! {
                        if let Some(api_key) = &self.api_key {
                            req = req.header(#h, format!(#format_str, api_key));
                        }
                    }
                }
            }
            None => quote! {
                if let Some(api_key) = &self.api_key {
                    req = req.bearer_auth(api_key);
                }
            },
        }
    }

    /// Generate header-parameter handling. Emits `req = req.header(name, ...)`
    /// for each `in: header` parameter — required headers unconditionally,
    /// optional ones gated on `Some(_)`.
    fn generate_header_params(&self, op: &OperationInfo) -> TokenStream {
        let header_params: Vec<_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "header")
            .collect();
        if header_params.is_empty() {
            return quote! {};
        }
        let mut emit = Vec::new();
        for param in header_params {
            let param_name_snake = self.operation_param_ident(op, param);
            let param_ident = Self::to_field_ident(&param_name_snake);
            let header_name = &param.name;
            if matches!(
                param.query_serialization,
                Some(crate::analysis::QuerySerialization::SimpleHeaderArray { .. })
            ) {
                let encode = quote! {
                    v.iter().map(::std::string::ToString::to_string).collect::<Vec<_>>().join(",")
                };
                if param.required {
                    emit.push(quote! {
                        let v = #param_ident;
                        req = req.header(#header_name, #encode);
                    });
                } else {
                    emit.push(quote! {
                        if let Some(v) = #param_ident {
                            req = req.header(#header_name, #encode);
                        }
                    });
                }
                continue;
            }
            if param.required {
                if Self::param_uses_as_ref_str(param) {
                    emit.push(quote! {
                        req = req.header(#header_name, #param_ident.as_ref());
                    });
                } else {
                    emit.push(quote! {
                        req = req.header(#header_name, #param_ident.to_string());
                    });
                }
            } else if Self::param_uses_as_ref_str(param) {
                emit.push(quote! {
                    if let Some(v) = #param_ident {
                        req = req.header(#header_name, v.as_ref());
                    }
                });
            } else {
                emit.push(quote! {
                    if let Some(v) = #param_ident {
                        req = req.header(#header_name, v.to_string());
                    }
                });
            }
        }
        quote! {
            #(#emit)*
        }
    }

    fn generate_cookie_params(&self, op: &OperationInfo) -> TokenStream {
        let cookie_params: Vec<_> = op
            .parameters
            .iter()
            .filter(|parameter| parameter.location == "cookie")
            .collect();
        if cookie_params.is_empty() {
            return quote! {};
        }
        let mut emit = Vec::new();
        for parameter in cookie_params {
            let ident = Self::to_field_ident(&self.operation_param_ident(op, parameter));
            let wire_name = parameter.name.as_str();
            if parameter.required {
                emit.push(quote! {
                    __cookie_fields.push(format!("{}={}", #wire_name, #ident));
                });
            } else {
                emit.push(quote! {
                    if let Some(value) = #ident {
                        __cookie_fields.push(format!("{}={}", #wire_name, value));
                    }
                });
            }
        }
        quote! {
            let mut __cookie_fields = Vec::new();
            #(#emit)*
            if !__cookie_fields.is_empty() {
                req = req.header(::reqwest::header::COOKIE, __cookie_fields.join("; "));
            }
        }
    }

    /// Generate query parameter handling
    fn generate_query_params(&self, op: &OperationInfo) -> TokenStream {
        let query_params: Vec<_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "query")
            .collect();

        if query_params.is_empty() {
            return quote! {};
        }

        let mut param_building = Vec::new();
        // Serialization applied on `req` directly, after the pair-vector
        // block: form-exploded objects and deepObject objects, whose keys
        // aren't the static parameter name.
        let mut req_appends = Vec::new();

        for param in query_params {
            use crate::analysis::QuerySerialization;

            // Use snake_case for Rust variable name with keyword escaping
            let param_name_snake = self.operation_param_ident(op, param);
            let param_name = Self::to_field_ident(&param_name_snake);

            // Use the original parameter name from OpenAPI spec as the query string key
            let param_key = &param.name;

            match &param.query_serialization {
                Some(QuerySerialization::FormExplodedNestedObject { properties }) => {
                    let emit_properties = properties.iter().map(|property| {
                        let wire_name = property.wire_name.as_str();
                        let field_ident = CodeGenerator::to_field_ident(
                            &self.to_rust_field_name(wire_name),
                        );
                        match &property.value_type {
                            crate::analysis::QueryStructPropertyType::Scalar(_) => quote! {
                                let value = serde_json::to_value(&v.#field_ident)
                                    .map_err(HttpError::serialization_error)?;
                                if !value.is_null() {
                                    let value = match value {
                                        serde_json::Value::String(value) => value,
                                        serde_json::Value::Bool(value) => value.to_string(),
                                        serde_json::Value::Number(value) => value.to_string(),
                                        _ => return Err(HttpError::serialization_error(
                                            format!("query field `{}` did not serialize as a scalar", #wire_name)
                                        ).into()),
                                    };
                                    nested_params.push((format!("{}.{}", #param_key, #wire_name), value));
                                }
                            },
                            crate::analysis::QueryStructPropertyType::Object { properties } => {
                                let leaves = properties.iter().map(|property| property.wire_name.clone()).collect::<Vec<_>>();
                                quote! {
                                    let value = serde_json::to_value(&v.#field_ident)
                                        .map_err(HttpError::serialization_error)?;
                                    if !value.is_null() {
                                        let serde_json::Value::Object(object) = value else {
                                            return Err(HttpError::serialization_error(format!("query field `{}` did not serialize as an object", #wire_name)).into());
                                        };
                                        if object.is_empty() {
                                            nested_params.push((format!("{}.{}[]", #param_key, #wire_name), String::new()));
                                        } else {
                                            for leaf in [#(#leaves),*] {
                                                let Some(value) = object.get(leaf) else { continue };
                                                if value.is_null() { continue; }
                                                let value = match value {
                                                    serde_json::Value::String(value) => value.clone(),
                                                    serde_json::Value::Bool(value) => value.to_string(),
                                                    serde_json::Value::Number(value) => value.to_string(),
                                                    _ => return Err(HttpError::serialization_error(format!("query field `{}` contained a non-scalar leaf", #wire_name)).into()),
                                                };
                                                nested_params.push((format!("{}.{}.{}", #param_key, #wire_name, leaf), value));
                                            }
                                        }
                                    }
                                }
                            }
                            crate::analysis::QueryStructPropertyType::Array { item_type } => {
                                let nested_properties = match item_type {
                                    crate::analysis::ArrayItemType::FlatStructRef { properties, .. } => Some(
                                        properties.iter().map(|property| property.wire_name.clone()).collect::<Vec<_>>()
                                    ),
                                    _ => None,
                                };
                                if let Some(nested_properties) = nested_properties {
                                    quote! {
                                        let values = serde_json::to_value(&v.#field_ident)
                                            .map_err(HttpError::serialization_error)?;
                                        if !values.is_null() {
                                            let serde_json::Value::Array(values) = values else {
                                                return Err(HttpError::serialization_error(
                                                    format!("query field `{}` did not serialize as an array", #wire_name)
                                                ).into());
                                            };
                                            if values.is_empty() {
                                                nested_params.push((format!("{}.{}[]", #param_key, #wire_name), String::new()));
                                            }
                                            for (index, value) in values.into_iter().enumerate() {
                                                let serde_json::Value::Object(object) = value else {
                                                    return Err(HttpError::serialization_error(
                                                        format!("query field `{}` contained a non-object item", #wire_name)
                                                    ).into());
                                                };
                                                for leaf in [#(#nested_properties),*] {
                                                    let Some(value) = object.get(leaf) else { continue };
                                                    if value.is_null() { continue; }
                                                    let value = match value {
                                                        serde_json::Value::String(value) => value.clone(),
                                                        serde_json::Value::Bool(value) => value.to_string(),
                                                        serde_json::Value::Number(value) => value.to_string(),
                                                        _ => return Err(HttpError::serialization_error(
                                                            format!("query field `{}` contained a non-scalar leaf", #wire_name)
                                                        ).into()),
                                                    };
                                                    nested_params.push((format!("{}.{}.{}.{}", #param_key, #wire_name, index + 1, leaf), value));
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    quote! {
                                        let values = serde_json::to_value(&v.#field_ident)
                                            .map_err(HttpError::serialization_error)?;
                                        if !values.is_null() {
                                            let serde_json::Value::Array(values) = values else {
                                                return Err(HttpError::serialization_error(
                                                    format!("query field `{}` did not serialize as an array", #wire_name)
                                                ).into());
                                            };
                                            if values.is_empty() {
                                                nested_params.push((format!("{}.{}[]", #param_key, #wire_name), String::new()));
                                            }
                                            for (index, value) in values.into_iter().enumerate() {
                                                let value = match value {
                                                    serde_json::Value::String(value) => value,
                                                    serde_json::Value::Bool(value) => value.to_string(),
                                                    serde_json::Value::Number(value) => value.to_string(),
                                                    _ => return Err(HttpError::serialization_error(
                                                        format!("query field `{}` contained a non-scalar item", #wire_name)
                                                    ).into()),
                                                };
                                                nested_params.push((format!("{}.{}.{}", #param_key, #wire_name, index + 1), value));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }).collect::<Vec<_>>();
                    let apply = quote! {
                        let mut nested_params: Vec<(String, String)> = Vec::new();
                        #(#emit_properties)*
                        if nested_params.is_empty() {
                            nested_params.push((format!("{}[]", #param_key), String::new()));
                        }
                        req = req.query(&nested_params);
                    };
                    if param.required {
                        req_appends.push(quote! {{ let v = #param_name; #apply }});
                    } else {
                        req_appends.push(quote! { if let Some(v) = #param_name { #apply } });
                    }
                    continue;
                }
                Some(QuerySerialization::FormExplodedObject) => {
                    // Issue #27: reqwest serializes the struct through
                    // serde_urlencoded, so each property becomes its own
                    // `key=value` pair; the parameter's own name never
                    // appears in the query string (RFC 6570 form-explosion).
                    // `name[]=` is the shared zero-cardinality marker used to
                    // preserve Some(empty) and required-empty values.
                    let apply = quote! {
                        let __empty = match serde_json::to_value(&v)
                            .map_err(HttpError::serialization_error)?
                        {
                            serde_json::Value::Object(map) => map.is_empty(),
                            _ => false,
                        };
                        if __empty {
                            req = req.query(&[(format!("{}[]", #param_key), String::new())]);
                        } else {
                            req = req.query(&v);
                        }
                    };
                    if param.required {
                        req_appends.push(quote! {
                            {
                                let v = #param_name;
                                #apply
                            }
                        });
                    } else {
                        req_appends.push(quote! {
                            if let Some(v) = #param_name {
                                #apply
                            }
                        });
                    }
                    continue;
                }
                Some(QuerySerialization::DeepObject) => {
                    // `?filter[color]=red&filter[size]=5`. Property values
                    // stringify through their JSON form; Null (unset
                    // Option) properties are skipped.
                    let apply = quote! {
                        let map = match serde_json::to_value(&v)
                            .map_err(HttpError::serialization_error)?
                        {
                            serde_json::Value::Object(map) => map,
                            _ => return Err(HttpError::serialization_error(
                                format!("query parameter `{}` did not serialize as an object", #param_key)
                            ).into()),
                        };
                        let mut deep_params: Vec<(String, String)> = Vec::new();
                        for (k, val) in map {
                            let s = match val {
                                serde_json::Value::Null => continue,
                                serde_json::Value::String(s) => s,
                                other => other.to_string(),
                            };
                            deep_params.push((format!("{}[{}]", #param_key, k), s));
                        }
                        if deep_params.is_empty() {
                            deep_params.push((format!("{}[]", #param_key), String::new()));
                        }
                        req = req.query(&deep_params);
                    };
                    if param.required {
                        req_appends.push(quote! {
                            {
                                let v = #param_name;
                                #apply
                            }
                        });
                    } else {
                        req_appends.push(quote! {
                            if let Some(v) = #param_name {
                                #apply
                            }
                        });
                    }
                    continue;
                }
                Some(QuerySerialization::FormObject) => {
                    // `?filter=color,red,size,big` — one pair whose value is
                    // the comma-joined key,value list (RFC 6570 form,
                    // explode=false).
                    let apply = quote! {
                        let map = match serde_json::to_value(&v)
                            .map_err(HttpError::serialization_error)?
                        {
                            serde_json::Value::Object(map) => map,
                            _ => return Err(HttpError::serialization_error(
                                format!("query parameter `{}` did not serialize as an object", #param_key)
                            ).into()),
                        };
                        let mut parts: Vec<String> = Vec::new();
                        for (k, val) in map {
                            let s = match val {
                                serde_json::Value::Null => continue,
                                serde_json::Value::String(s) => s,
                                other => other.to_string(),
                            };
                            if k.contains(',') || s.contains(',') {
                                return Err(HttpError::serialization_error(
                                    format!(
                                        "query object `{}` contains a comma in key `{}`; use explode=true for lossless string values",
                                        #param_key,
                                        k,
                                    )
                                ).into());
                            }
                            parts.push(k);
                            parts.push(s);
                        }
                        if parts.is_empty() {
                            query_params.push((
                                format!("{}[]", #param_key),
                                String::new(),
                            ));
                        } else {
                            query_params.push((#param_key.to_string(), parts.join(",")));
                        }
                    };
                    if param.required {
                        param_building.push(quote! {
                            {
                                let v = #param_name;
                                #apply
                            }
                        });
                    } else {
                        param_building.push(quote! {
                            if let Some(v) = #param_name {
                                #apply
                            }
                        });
                    }
                    continue;
                }
                Some(QuerySerialization::FormExplodedArray { item_type }) => {
                    // `?tags=a&tags=b` — one pair per element; flat structures
                    // expand AWS query-protocol style as `?tags.1.Key=k&tags.1.Value=v`.
                    let struct_properties = match item_type {
                        crate::analysis::ArrayItemType::FlatStructRef { properties, .. }
                        | crate::analysis::ArrayItemType::NestedStructRef { properties, .. } => {
                            Some(properties.clone())
                        }
                        _ => None,
                    };
                    let emit_items = if let Some(properties) = struct_properties {
                        let pushes = properties
                            .iter()
                            .map(|property| {
                                let wire_name = &property.wire_name;
                                // Wire names such as `Type` land on struct
                                // fields via the same keyword-escaping the
                                // model generator uses (`r#type`).
                                let field_ident = CodeGenerator::to_field_ident(
                                    &self.to_rust_field_name(wire_name),
                                );
                                match &property.value_type {
                                    crate::analysis::QueryStructPropertyType::Scalar(_) => quote! {
                                        let value = serde_json::to_value(&item.#field_ident)
                                            .map_err(HttpError::serialization_error)?;
                                        if !value.is_null() {
                                            let value = match value {
                                                serde_json::Value::String(value) => value,
                                                serde_json::Value::Bool(value) => value.to_string(),
                                                serde_json::Value::Number(value) => value.to_string(),
                                                _ => return Err(HttpError::serialization_error(
                                                    format!("query field `{}.{}` did not serialize as a scalar", #param_key, #wire_name)
                                                ).into()),
                                            };
                                            query_params.push((
                                                format!("{}.{}.{}", #param_key, index, #wire_name),
                                                value,
                                            ));
                                        }
                                    },
                                    crate::analysis::QueryStructPropertyType::Object { properties } => {
                                        let leaves = properties.iter().map(|property| property.wire_name.clone()).collect::<Vec<_>>();
                                        quote! {
                                            let value = serde_json::to_value(&item.#field_ident)
                                                .map_err(HttpError::serialization_error)?;
                                            if !value.is_null() {
                                                let serde_json::Value::Object(object) = value else {
                                                    return Err(HttpError::serialization_error(format!("query field `{}.{}` did not serialize as an object", #param_key, #wire_name)).into());
                                                };
                                                if object.is_empty() {
                                                    query_params.push((format!("{}.{}.{}[]", #param_key, index, #wire_name), String::new()));
                                                }
                                                for leaf in [#(#leaves),*] {
                                                    let Some(value) = object.get(leaf) else { continue };
                                                    if value.is_null() { continue; }
                                                    let value = match value {
                                                        serde_json::Value::String(value) => value.clone(),
                                                        serde_json::Value::Bool(value) => value.to_string(),
                                                        serde_json::Value::Number(value) => value.to_string(),
                                                        _ => return Err(HttpError::serialization_error(format!("query field `{}.{}` contained a non-scalar leaf", #param_key, #wire_name)).into()),
                                                    };
                                                    query_params.push((format!("{}.{}.{}.{}", #param_key, index, #wire_name, leaf), value));
                                                }
                                            }
                                        }
                                    }
                                    crate::analysis::QueryStructPropertyType::Array { item_type } => {
                                        let nested_properties = match item_type {
                                            crate::analysis::ArrayItemType::FlatStructRef { properties, .. } => {
                                                Some(properties.iter().map(|property| property.wire_name.clone()).collect::<Vec<_>>())
                                            }
                                            _ => None,
                                        };
                                        if let Some(nested_properties) = nested_properties {
                                            quote! {
                                                let values = serde_json::to_value(&item.#field_ident)
                                                    .map_err(HttpError::serialization_error)?;
                                                if !values.is_null() {
                                                    let serde_json::Value::Array(values) = values else {
                                                        return Err(HttpError::serialization_error(
                                                            format!("query field `{}.{}` did not serialize as an array", #param_key, #wire_name)
                                                        ).into());
                                                    };
                                                    if values.is_empty() {
                                                        query_params.push((format!("{}.{}.{}[]", #param_key, index, #wire_name), String::new()));
                                                    }
                                                    for (nested_index, value) in values.into_iter().enumerate() {
                                                        let serde_json::Value::Object(object) = value else {
                                                            return Err(HttpError::serialization_error(
                                                                format!("query field `{}.{}` contained a non-object item", #param_key, #wire_name)
                                                            ).into());
                                                        };
                                                        if object.is_empty() {
                                                            query_params.push((format!("{}.{}.{}.{}[]", #param_key, index, #wire_name, nested_index + 1), String::new()));
                                                        }
                                                        for nested_wire_name in [#(#nested_properties),*] {
                                                            let Some(value) = object.get(nested_wire_name) else { continue };
                                                            if value.is_null() { continue; }
                                                            let value = match value {
                                                                serde_json::Value::String(value) => value.clone(),
                                                                serde_json::Value::Bool(value) => value.to_string(),
                                                                serde_json::Value::Number(value) => value.to_string(),
                                                                _ => return Err(HttpError::serialization_error(
                                                                    format!("query field `{}.{}` contained a non-scalar leaf", #param_key, #wire_name)
                                                                ).into()),
                                                            };
                                                            query_params.push((
                                                                format!("{}.{}.{}.{}.{}", #param_key, index, #wire_name, nested_index + 1, nested_wire_name),
                                                                value,
                                                            ));
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            quote! {
                                                let values = serde_json::to_value(&item.#field_ident)
                                                    .map_err(HttpError::serialization_error)?;
                                                if !values.is_null() {
                                                    let serde_json::Value::Array(values) = values else {
                                                        return Err(HttpError::serialization_error(
                                                            format!("query field `{}.{}` did not serialize as an array", #param_key, #wire_name)
                                                        ).into());
                                                    };
                                                    if values.is_empty() {
                                                        query_params.push((format!("{}.{}.{}[]", #param_key, index, #wire_name), String::new()));
                                                    }
                                                    for (nested_index, value) in values.into_iter().enumerate() {
                                                        let value = match value {
                                                            serde_json::Value::String(value) => value,
                                                            serde_json::Value::Bool(value) => value.to_string(),
                                                            serde_json::Value::Number(value) => value.to_string(),
                                                            _ => return Err(HttpError::serialization_error(
                                                                format!("query field `{}.{}` contained a non-scalar item", #param_key, #wire_name)
                                                            ).into()),
                                                        };
                                                        query_params.push((
                                                            format!("{}.{}.{}.{}", #param_key, index, #wire_name, nested_index + 1),
                                                            value,
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            })
                            .collect::<Vec<_>>();
                        quote! {
                            for (index, item) in v.iter().enumerate() {
                                let index = index + 1;
                                let item_start_len = query_params.len();
                                #(#pushes)*
                                if query_params.len() == item_start_len {
                                    query_params.push((format!("{}.{}[]", #param_key, index), String::new()));
                                }
                            }
                        }
                    } else {
                        quote! {
                            for item in v {
                                query_params.push((#param_key.to_string(), item.to_string()));
                            }
                        }
                    };
                    if param.required {
                        param_building.push(quote! {
                            let v = #param_name;
                            if v.is_empty() {
                                query_params.push((
                                    format!("{}[]", #param_key),
                                    String::new(),
                                ));
                            } else {
                                #emit_items
                            }
                        });
                    } else {
                        param_building.push(quote! {
                            if let Some(v) = #param_name {
                                if v.is_empty() {
                                    query_params.push((
                                        format!("{}[]", #param_key),
                                        String::new(),
                                    ));
                                } else {
                                    #emit_items
                                }
                            }
                        });
                    }
                    continue;
                }
                Some(QuerySerialization::FormArray { .. }) => {
                    // `?tags=a,b,c` — one comma-joined pair. Empty vectors
                    // use the shared `tags[]=` zero-cardinality marker.
                    let apply = quote! {
                        if v.is_empty() {
                            query_params.push((
                                format!("{}[]", #param_key),
                                String::new(),
                            ));
                        } else {
                            let mut parts = Vec::with_capacity(v.len());
                            for item in &v {
                                let item = item.to_string();
                                if item.contains(',') {
                                    return Err(HttpError::serialization_error(
                                        format!(
                                            "query array `{}` contains a comma; use explode=true for lossless string values",
                                            #param_key,
                                        )
                                    ).into());
                                }
                                parts.push(item);
                            }
                            query_params.push((
                                #param_key.to_string(),
                                parts.join(","),
                            ));
                        }
                    };
                    if param.required {
                        param_building.push(quote! {
                            {
                                let v = #param_name;
                                #apply
                            }
                        });
                    } else {
                        param_building.push(quote! {
                            if let Some(v) = #param_name {
                                #apply
                            }
                        });
                    }
                    continue;
                }
                Some(
                    QuerySerialization::Unsupported { .. }
                    | QuerySerialization::SimpleHeaderArray { .. },
                ) => {}
                None => {}
            }

            if param.required {
                // Required parameters: always add
                if Self::param_uses_as_ref_str(param) {
                    param_building.push(quote! {
                        query_params.push((#param_key.to_string(), #param_name.as_ref().to_string()));
                    });
                } else {
                    param_building.push(quote! {
                        query_params.push((#param_key.to_string(), #param_name.to_string()));
                    });
                }
            } else {
                // Optional parameters: add only if Some
                if Self::param_uses_as_ref_str(param) {
                    param_building.push(quote! {
                        if let Some(v) = #param_name {
                            query_params.push((#param_key.to_string(), v.as_ref().to_string()));
                        }
                    });
                } else {
                    param_building.push(quote! {
                        if let Some(v) = #param_name {
                            query_params.push((#param_key.to_string(), v.to_string()));
                        }
                    });
                }
            }
        }

        // Ops whose query params all serialize on `req` directly skip the
        // pair-vector block entirely.
        let pairs_block = if param_building.is_empty() {
            quote! {}
        } else {
            quote! {
                {
                    let mut query_params: Vec<(String, String)> = Vec::new();
                    #(#param_building)*
                    if !query_params.is_empty() {
                        req = req.query(&query_params);
                    }
                }
            }
        };

        quote! {
            // Add query parameters
            #pairs_block
            #(#req_appends)*
        }
    }

    /// Generate the rustdoc block for an operation, surfacing summary,
    /// description, the HTTP method+path, and any tags from the OAS spec
    /// (T13). Also marks the method `#[deprecated]` if the operation is.
    fn generate_operation_doc_comment(&self, op: &OperationInfo) -> TokenStream {
        let method = op.method.to_uppercase();
        let path = &op.path;
        let mut docs: Vec<String> = Vec::new();
        if let Some(s) = &op.summary {
            if !s.is_empty() {
                docs.push(s.clone());
                docs.push(String::new());
            }
        }
        if let Some(d) = &op.description {
            if !d.is_empty() {
                for line in d.lines() {
                    docs.push(line.to_string());
                }
                docs.push(String::new());
            }
        }
        docs.push(format!("`{} {}`", method, path));
        let doc_attrs: Vec<TokenStream> = docs
            .iter()
            .map(|line| {
                let prefixed = if line.is_empty() {
                    String::new()
                } else {
                    format!(" {line}")
                };
                quote! { #[doc = #prefixed] }
            })
            .collect();
        quote! { #(#doc_attrs)* }
    }

    /// Get the method name from the operation
    fn get_method_name(&self, op: &OperationInfo) -> syn::Ident {
        let name = if !op.operation_id.is_empty() {
            op.operation_id.to_snake_case()
        } else {
            // Fallback: generate from HTTP method and path
            format!(
                "{}_{}",
                op.method,
                op.path.replace('/', "_").replace(['{', '}'], "")
            )
            .to_snake_case()
        };

        Self::to_field_ident(&self.escape_keyword_ident(&name))
    }

    /// Build the request-builder expression for the operation's HTTP method.
    /// Named reqwest methods (`.get`/`.post`/…) are used where available;
    /// OPTIONS and TRACE go through `Client::request(Method::OPTIONS, _)` since
    /// reqwest doesn't expose those as named methods.
    fn http_method_call(&self, op: &OperationInfo) -> TokenStream {
        match op.method.to_uppercase().as_str() {
            "GET" => quote! { self.http_client.get(request_url) },
            "POST" => quote! { self.http_client.post(request_url) },
            "PUT" => quote! { self.http_client.put(request_url) },
            "DELETE" => quote! { self.http_client.delete(request_url) },
            "PATCH" => quote! { self.http_client.patch(request_url) },
            "HEAD" => quote! { self.http_client.head(request_url) },
            "OPTIONS" => quote! {
                self.http_client.request(reqwest::Method::OPTIONS, request_url)
            },
            "TRACE" => quote! {
                self.http_client.request(reqwest::Method::TRACE, request_url)
            },
            // D1: 3.2 `QUERY` verb + any custom verb from
            // PathItem.additionalOperations. reqwest's Method::from_bytes
            // accepts arbitrary uppercase tokens that match the RFC7230
            // method grammar.
            other => {
                let upper = other.to_string();
                quote! {
                    self.http_client.request(
                        reqwest::Method::from_bytes(#upper.as_bytes())
                            .expect("invalid HTTP method"),
                        request_url,
                    )
                }
            }
        }
    }

    /// Generate request parameters including path, query, header, and request body.
    fn generate_request_param(&self, op: &OperationInfo) -> TokenStream {
        self.generate_request_param_with_capture(op, None)
    }

    fn generate_request_param_with_capture(
        &self,
        op: &OperationInfo,
        mut captures: Option<&mut Vec<syn::Ident>>,
    ) -> TokenStream {
        let mut params = Vec::new();
        for allocated in self.allocated_operation_params(op) {
            let param_name = allocated.ident;
            let param_type = if Self::param_has_impl_as_ref_type(allocated.param) {
                if let Some(captures) = captures.as_deref_mut() {
                    let ident = format_ident!("__Param{}", captures.len());
                    captures.push(ident.clone());
                    quote! { #ident }
                } else {
                    self.get_param_rust_type(allocated.param)
                }
            } else {
                self.get_param_rust_type(allocated.param)
            };
            if Self::builder_param_is_required(allocated.param) {
                params.push(quote! { #param_name: #param_type });
            } else {
                params.push(quote! { #param_name: Option<#param_type> });
            }
        }

        // Add request body parameter based on content type. Optional bodies
        // (`requestBody.required` is false or absent) become `Option<T>` per T11.
        if let Some(ref rb) = op.request_body {
            use crate::analysis::RequestBodyContent;
            if matches!(rb, RequestBodyContent::SchemaLess { .. }) {
                return if params.is_empty() {
                    quote! {}
                } else {
                    quote! { #(#params),* }
                };
            }
            let required = op.request_body_required;
            let body_type = match rb {
                RequestBodyContent::Json { schema_name, .. }
                | RequestBodyContent::FormUrlEncoded { schema_name, .. }
                | RequestBodyContent::Multipart { schema_name, .. } => {
                    let rust_type_name = self.to_rust_type_name(schema_name);
                    let request_ident =
                        syn::Ident::new(&rust_type_name, proc_macro2::Span::call_site());
                    quote! { #request_ident }
                }

                RequestBodyContent::OctetStream { .. } | RequestBodyContent::Binary { .. } => {
                    quote! { Vec<u8> }
                }
                RequestBodyContent::TextPlain { .. } => quote! { String },
                RequestBodyContent::Unsupported { .. } => quote! { Vec<u8> },
                RequestBodyContent::SchemaLess { .. } => unreachable!(
                    "schema-less request bodies preserve the historical client signature"
                ),
            };
            let body_ident = match rb {
                RequestBodyContent::OctetStream { .. }
                | RequestBodyContent::Binary { .. }
                | RequestBodyContent::TextPlain { .. }
                | RequestBodyContent::Unsupported { .. } => quote! { body },
                RequestBodyContent::SchemaLess { .. } => unreachable!(
                    "schema-less request bodies preserve the historical client signature"
                ),
                _ => quote! { request },
            };
            if required {
                params.push(quote! { #body_ident: #body_type });
            } else {
                params.push(quote! { #body_ident: Option<#body_type> });
            }
        }

        if params.is_empty() {
            quote! {}
        } else {
            quote! { #(#params),* }
        }
    }

    /// Get the Rust type for a parameter
    fn get_param_rust_type(&self, param: &crate::analysis::ParameterInfo) -> TokenStream {
        if Self::param_has_impl_as_ref_type(param) {
            quote! { impl AsRef<str> }
        } else {
            self.get_param_owned_rust_type(param)
        }
    }

    /// Owned parameter type shared by client-builder storage and generated
    /// server extraction. [`ParameterInfo::query_serialization`] is the
    /// authoritative projection for typed query objects and arrays.
    pub(crate) fn get_param_owned_rust_type(
        &self,
        param: &crate::analysis::ParameterInfo,
    ) -> TokenStream {
        use crate::analysis::QuerySerialization;
        // Typed form-style arrays take Vec<item> (openapi-generator-anu).
        // Scalars parse as-is (they may be type paths from [type_mappings]);
        // schema refs are raw schema names and go through the same
        // to_rust_type_name sanitization as every other schema reference
        // (cloudflare has enum schemas like `resource-sharing_resource_type`).
        if let Some(
            QuerySerialization::FormExplodedArray { item_type }
            | QuerySerialization::FormArray { item_type }
            | QuerySerialization::SimpleHeaderArray { item_type },
        ) = &param.query_serialization
        {
            use crate::analysis::ArrayItemType;
            let item_ty: syn::Type = match item_type {
                ArrayItemType::Scalar(rust_type) => syn::parse_str(rust_type)
                    .unwrap_or_else(|_| panic!("invalid scalar item type `{rust_type}`")),
                ArrayItemType::SchemaRef(schema_name) => {
                    let rust_name = self.to_rust_type_name(schema_name);
                    syn::parse_str(&rust_name)
                        .unwrap_or_else(|_| panic!("invalid schema item type `{rust_name}`"))
                }
                ArrayItemType::FlatStructRef { schema_name, .. } => {
                    let rust_name = self.to_rust_type_name(schema_name);
                    syn::parse_str(&rust_name)
                        .unwrap_or_else(|_| panic!("invalid struct item type `{rust_name}`"))
                }
                ArrayItemType::NestedStructRef { schema_name, .. } => {
                    let rust_name = self.to_rust_type_name(schema_name);
                    syn::parse_str(&rust_name)
                        .unwrap_or_else(|_| panic!("invalid nested struct item type `{rust_name}`"))
                }
            };
            return quote! { Vec<#item_ty> };
        }
        // T10: $ref-typed parameters used to lose their type because we only
        // consulted `rust_type` (which stays "String"). Now: prefer the
        // resolved schema reference if present.
        if let Some(ref schema_name) = param.schema_ref {
            let rust_name = self.to_rust_type_name(schema_name);
            let ident = syn::Ident::new(&rust_name, proc_macro2::Span::call_site());
            return quote! { #ident };
        }
        syn::parse_str::<syn::Type>(&param.rust_type)
            .map(|ty| quote! { #ty })
            .unwrap_or_else(|_| {
                let type_ident = syn::Ident::new(&param.rust_type, proc_macro2::Span::call_site());
                quote! { #type_ident }
            })
    }

    /// True when the parameter's compile-time type is `impl AsRef<str>` and
    /// we should call `.as_ref()` on it before stringifying. False for any
    /// $ref-resolved type (T10) or non-String primitive — those just call
    /// `.to_string()`.
    fn param_uses_as_ref_str(param: &crate::analysis::ParameterInfo) -> bool {
        param.schema_ref.is_none() && param.rust_type == "String"
    }

    fn resolve_multipart_wire_schema<'a>(
        schema: &'a serde_json::Value,
        analysis: &'a SchemaAnalysis,
        visited: &mut std::collections::HashSet<String>,
    ) -> Option<&'a serde_json::Value> {
        let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) else {
            for keyword in ["anyOf", "oneOf"] {
                if let Some(branches) = schema.get(keyword).and_then(serde_json::Value::as_array) {
                    let concrete: Vec<_> = branches
                        .iter()
                        .filter(|branch| {
                            branch.get("type").and_then(serde_json::Value::as_str) != Some("null")
                        })
                        .collect();
                    if concrete.len() == 1 && concrete.len() < branches.len() {
                        return Self::resolve_multipart_wire_schema(concrete[0], analysis, visited);
                    }
                }
            }
            if let Some(branches) = schema.get("allOf").and_then(serde_json::Value::as_array) {
                if branches.len() == 1 {
                    return Self::resolve_multipart_wire_schema(&branches[0], analysis, visited);
                }
            }
            return Some(schema);
        };
        let name = reference.strip_prefix("#/components/schemas/")?;
        if !visited.insert(name.to_string()) {
            return None;
        }
        let resolved = analysis.validation_context.component_schemas.get(name)?;
        Self::resolve_multipart_wire_schema(resolved, analysis, visited)
    }

    fn multipart_client_field_kind(
        schema_type: &crate::analysis::SchemaType,
        analysis: &SchemaAnalysis,
        visited: &mut std::collections::HashSet<String>,
    ) -> Option<MultipartClientFieldKind> {
        match schema_type {
            crate::analysis::SchemaType::Primitive {
                rust_type,
                serde_with,
            } => {
                let rust_type = rust_type.replace(' ', "");
                if rust_type == "bytes::Bytes" {
                    Some(MultipartClientFieldKind::RawBytes)
                } else if serde_with
                    .as_deref()
                    .is_some_and(|codec| codec.contains("base64_url"))
                {
                    Some(MultipartClientFieldKind::Base64UrlUnpadded)
                } else if serde_with
                    .as_deref()
                    .is_some_and(|codec| codec.contains("base64"))
                    || rust_type == "Vec<u8>"
                {
                    Some(MultipartClientFieldKind::Base64)
                } else {
                    Some(MultipartClientFieldKind::Text)
                }
            }
            crate::analysis::SchemaType::StringEnum { .. }
            | crate::analysis::SchemaType::ExtensibleEnum { .. } => {
                Some(MultipartClientFieldKind::Text)
            }
            crate::analysis::SchemaType::Reference { target } => {
                if !visited.insert(target.clone()) {
                    return None;
                }
                analysis.schemas.get(target).and_then(|schema| {
                    Self::multipart_client_field_kind(&schema.schema_type, analysis, visited)
                })
            }
            _ => None,
        }
    }

    fn multipart_binary_fields(
        &self,
        operation: &OperationInfo,
        analysis: &SchemaAnalysis,
    ) -> Vec<String> {
        let Some(crate::analysis::RequestBodyContent::Multipart {
            validation_schema, ..
        }) = &operation.request_body
        else {
            return Vec::new();
        };
        let Some(schema) = Self::resolve_multipart_wire_schema(
            validation_schema,
            analysis,
            &mut std::collections::HashSet::new(),
        ) else {
            return Vec::new();
        };
        schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .into_iter()
            .flat_map(|p| p.iter())
            .filter_map(|(name, schema)| {
                let schema = Self::resolve_multipart_wire_schema(
                    schema,
                    analysis,
                    &mut std::collections::HashSet::new(),
                )?;
                (schema.get("format").and_then(serde_json::Value::as_str) == Some("binary"))
                    .then(|| name.clone())
            })
            .collect()
    }

    fn generate_multipart_filename_validation(
        &self,
        operation: &OperationInfo,
        analysis: &SchemaAnalysis,
        filenames: Option<&syn::Ident>,
    ) -> TokenStream {
        let Some(filenames) = filenames else {
            return TokenStream::new();
        };
        let keys = self.multipart_binary_fields(operation, analysis);
        quote! { {
            let mut seen = std::collections::HashSet::new();
            for &(key, _) in #filenames {
                if ![#(#keys),*].contains(&key) {
                    return Err(HttpError::Config(format!("multipart filename key `{}` is unknown or is not a binary field", key)).into());
                }
                if !seen.insert(key) {
                    return Err(HttpError::Config(format!("duplicate multipart filename key `{}`", key)).into());
                }
            }
        } }
    }

    fn generate_response_content_type_validation(
        &self,
        operation: &OperationInfo,
        success: &ClientSuccessSelection<'_>,
    ) -> TokenStream {
        let Some(expected) = success.accept else {
            return TokenStream::new();
        };
        let guard = self.success_selection_guard(operation, success);
        let errors = self.generate_error_match_arms(operation);
        let error_type = self.op_error_type_token(operation);
        let raw_body = if matches!(
            success.body,
            ClientSuccessBody::EventStream | ClientSuccessBody::ParsedSse
        ) {
            quote! { Vec::new() }
        } else {
            quote! { __read_bounded_response_body(response, self.max_response_body_bytes).await? }
        };
        quote! {
            let status = response.status();
            let status_code = status.as_u16();
            if #guard {
                let actual = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or("").to_string();
                let expected = #expected;
                let parse_media = |value: &str| {
                    let mut parts = Vec::new();
                    let mut start = 0;
                    let mut quoted = false;
                    let mut escaped = false;
                    for (index, character) in value.char_indices() {
                        if escaped { escaped = false; continue }
                        if character == '\\' && quoted { escaped = true; continue }
                        if character == '"' { quoted = !quoted; }
                        if character == ';' && !quoted { parts.push(&value[start..index]); start = index + 1; }
                    }
                    if quoted || escaped { return None }
                    parts.push(&value[start..]);
                    let essence = parts.first().copied().unwrap_or("").trim().to_ascii_lowercase();
                    let mut parameters = std::collections::BTreeMap::new();
                    for part in parts.into_iter().skip(1) {
                        let (name, value) = part.split_once('=')?;
                        let name = name.trim().to_ascii_lowercase();
                        if name.is_empty() { return None }
                        let value = value.trim();
                        let value = if let Some(quoted) = value.strip_prefix('"') {
                            let inner = quoted.strip_suffix('"')?;
                            let mut decoded = String::new();
                            let mut escaped = false;
                            for character in inner.chars() {
                                if escaped { decoded.push(character); escaped = false; }
                                else if character == '\\' { escaped = true; }
                                else if character == '"' { return None }
                                else { decoded.push(character); }
                            }
                            if escaped { return None }
                            decoded
                        } else {
                            if value.contains('"') { return None }
                            value.to_string()
                        };
                        if parameters.insert(name, value).is_some() { return None }
                    }
                    Some((essence, parameters))
                };
                let matches = match (parse_media(&actual), parse_media(expected)) {
                    (Some((actual_essence, actual_parameters)), Some((expected_essence, expected_parameters))) => {
                        let essence_matches = if expected_essence == "*/*" { actual_essence.contains('/') }
                            else if let Some(prefix) = expected_essence.strip_suffix("/*") { actual_essence.split('/').next() == Some(prefix) }
                            else { actual_essence == expected_essence };
                        essence_matches && expected_parameters.iter().all(|(name, value)| actual_parameters.get(name).is_some_and(|actual|
                            if name == "charset" { actual.eq_ignore_ascii_case(value) } else { actual == value }))
                    }
                    _ => false,
                };
                if !matches {
                    let headers = response.headers().clone();
                    let raw_body = #raw_body;
                    let body_text = String::from_utf8_lossy(&raw_body).into_owned();
                    let typed: Option<#error_type>;
                    let parse_error: Option<String>;
                    #errors
                    let _ = parse_error;
                    return Err(ApiOpError::Api(ApiError {
                        status: status_code, headers, body: body_text, raw_body, typed,
                        parse_error: Some(format!("unexpected response Content-Type `{}`; expected `{}`", actual, expected)),
                    }));
                }
            }
        }
    }

    fn generate_typed_multipart_form(
        &self,
        schema_name: &str,
        validation_schema: &serde_json::Value,
        analysis: &SchemaAnalysis,
        filenames: Option<&syn::Ident>,
    ) -> TokenStream {
        use crate::analysis::{ObjectAdditionalProperties, SchemaType};

        let Some((resolved_name, resolved_schema)) =
            self.resolve_reference_schema(schema_name, analysis)
        else {
            let message = format!(
                "multipart request schema `{schema_name}` could not be resolved during generation"
            );
            return quote! {
                return Err(HttpError::Config(#message.to_string()).into());
            };
        };
        let SchemaType::Object {
            properties,
            required,
            additional_properties,
            ..
        } = &resolved_schema.schema_type
        else {
            let message =
                format!("multipart request schema `{schema_name}` must resolve to an object");
            return quote! {
                return Err(HttpError::Config(#message.to_string()).into());
            };
        };
        let wire_schema = Self::resolve_multipart_wire_schema(
            validation_schema,
            analysis,
            &mut std::collections::HashSet::new(),
        );
        let wire_properties = wire_schema
            .and_then(|schema| schema.get("properties"))
            .and_then(serde_json::Value::as_object);
        let fields = self.emitted_object_properties(
            resolved_name,
            properties,
            required,
            additional_properties,
            analysis,
        );
        if matches!(
            additional_properties,
            ObjectAdditionalProperties::Typed { .. }
        ) {
            let message = format!(
                "multipart request schema `{schema_name}` cannot contain typed additional properties"
            );
            return quote! {
                return Err(HttpError::Config(#message.to_string()).into());
            };
        }

        let mut parts = Vec::new();
        for field in fields {
            let wire_name = field.wire_name;
            let is_nullable = self.property_is_nullable(resolved_name, wire_name, field.property);
            let is_tri_state = self.property_is_tri_state(
                resolved_name,
                wire_name,
                field.property,
                field.is_required,
            );
            let ident = field.ident;
            let wire_format = wire_properties
                .and_then(|properties| properties.get(wire_name))
                .and_then(|schema| {
                    Self::resolve_multipart_wire_schema(
                        schema,
                        analysis,
                        &mut std::collections::HashSet::new(),
                    )
                })
                .and_then(|schema| schema.get("format"))
                .and_then(serde_json::Value::as_str);
            let kind = if wire_format == Some("binary") {
                match self.config().types.binary {
                    crate::type_mapping::BinaryStrategy::String => MultipartClientFieldKind::Text,
                    crate::type_mapping::BinaryStrategy::Bytes
                    | crate::type_mapping::BinaryStrategy::VecU8 => {
                        MultipartClientFieldKind::RawBytes
                    }
                }
            } else if let Some(kind) = Self::multipart_client_field_kind(
                &field.property.schema_type,
                analysis,
                &mut std::collections::HashSet::new(),
            ) {
                kind
            } else {
                let message =
                    format!("multipart field `{wire_name}` must be binary or a scalar text field");
                return quote! {
                    return Err(HttpError::Config(#message.to_string()).into());
                };
            };
            let filename_assignment = filenames.map(|filenames| quote! {
                if let Some((_, filename)) = #filenames.iter().find(|(key, _)| *key == #wire_name) {
                    part = part.file_name((*filename).to_string());
                }
            }).unwrap_or_default();
            let add_value = if wire_format == Some("binary") && filenames.is_some() {
                let part = match kind {
                    MultipartClientFieldKind::RawBytes => {
                        quote! { reqwest::multipart::Part::bytes(value.to_vec()) }
                    }
                    _ => quote! { reqwest::multipart::Part::text(value.to_string()) },
                };
                quote! {
                    let mut part = #part;
                    #filename_assignment
                    form = form.part(#wire_name, part);
                }
            } else {
                match kind {
                    MultipartClientFieldKind::RawBytes => quote! {
                        form = form.part(#wire_name, reqwest::multipart::Part::bytes(value.to_vec()));
                    },
                    MultipartClientFieldKind::Base64 => quote! {
                        use base64::Engine as _;
                        form = form.text(#wire_name, base64::engine::general_purpose::STANDARD.encode(value));
                    },
                    MultipartClientFieldKind::Base64UrlUnpadded => quote! {
                        use base64::Engine as _;
                        form = form.text(#wire_name, base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value));
                    },
                    MultipartClientFieldKind::Text => {
                        quote! { form = form.text(#wire_name, value.to_string()); }
                    }
                }
            };
            parts.push(if is_tri_state {
                // Multipart has no representation for a JSON null part. A
                // present concrete value is sent; both outer absence and an
                // explicit-null model state omit the part.
                quote! {
                    if let Some(Some(value)) = &request.#ident {
                        #add_value
                    }
                }
            } else if field.is_required && is_nullable {
                quote! {
                    if let Some(value) = &request.#ident {
                        #add_value
                    }
                }
            } else if field.is_required {
                quote! {
                    let value = &request.#ident;
                    #add_value
                }
            } else {
                quote! {
                    if let Some(value) = &request.#ident {
                        #add_value
                    }
                }
            });
        }

        quote! {
            let mut form = reqwest::multipart::Form::new();
            #(#parts)*
            req = req.multipart(form);
        }
    }

    /// Generate request body serialization based on content type
    /// Emit statements that mutate `req` to apply the request body. Returns
    /// explicit zero-length framing for bodyless POST, PUT, and PATCH requests.
    /// Optional bodies (T11) gate the application on `Some(_)`; required bodies
    /// apply unconditionally.
    fn generate_request_body_with_filenames(
        &self,
        op: &OperationInfo,
        analysis: &SchemaAnalysis,
        filenames: Option<&syn::Ident>,
    ) -> TokenStream {
        let empty_request_framing = Self::generate_empty_request_framing(op);
        let Some(rb) = op.request_body.as_ref() else {
            return empty_request_framing;
        };
        use crate::analysis::RequestBodyContent;
        let required = op.request_body_required;
        let (ident, apply): (TokenStream, TokenStream) = match rb {
            RequestBodyContent::Json { media_type, .. } => (
                quote! { request },
                quote! {
                    req = req
                        .body(serde_json::to_vec(&request).map_err(HttpError::serialization_error)?)
                        .header("content-type", #media_type);
                },
            ),
            RequestBodyContent::FormUrlEncoded { media_type, .. } => (
                quote! { request },
                quote! {
                    req = req
                        .body(serde_urlencoded::to_string(&request).map_err(HttpError::serialization_error)?)
                        .header("content-type", #media_type);
                },
            ),
            RequestBodyContent::Multipart {
                schema_name,
                validation_schema,
                ..
            } => (
                quote! { request },
                self.generate_typed_multipart_form(
                    schema_name,
                    validation_schema,
                    analysis,
                    filenames,
                ),
            ),
            RequestBodyContent::OctetStream { media_type } => (
                quote! { body },
                quote! {
                    req = req
                        .body(body)
                        .header("content-type", #media_type);
                },
            ),
            RequestBodyContent::Binary { media_type } => (
                quote! { body },
                quote! {
                    req = req
                        .body(body)
                        .header("content-type", #media_type);
                },
            ),
            RequestBodyContent::TextPlain { media_type } => (
                quote! { body },
                quote! {
                    req = req
                        .body(body)
                        .header("content-type", #media_type);
                },
            ),
            RequestBodyContent::Unsupported { media_types } => {
                let media_type = media_types
                    .iter()
                    .find(|media_type| !crate::openapi::is_wildcard_media_type(media_type))
                    .map(String::as_str);
                let Some(media_type) = media_type else {
                    let ranges = media_types.join(", ");
                    let message = format!(
                        "request body for operation `{}` declares only wildcard media ranges ({ranges}); a concrete Content-Type is required",
                        op.operation_id,
                    );
                    return if required {
                        quote! {
                            let _ = body;
                            return Err(HttpError::Config(#message.to_string()).into());
                        }
                    } else {
                        quote! {
                            if body.is_some() {
                                return Err(HttpError::Config(#message.to_string()).into());
                            }
                            #empty_request_framing
                        }
                    };
                };
                (
                    quote! { body },
                    quote! {
                        req = req
                            .body(body)
                            .header("content-type", #media_type);
                    },
                )
            }
            RequestBodyContent::SchemaLess { .. } => return empty_request_framing,
        };
        if required {
            apply
        } else {
            quote! {
                if let Some(#ident) = #ident {
                    #apply
                } else {
                    #empty_request_framing
                }
            }
        }
    }

    /// Emit explicit HTTP/1.1 framing when an operation sends no request body.
    /// RFC 9110 recommends `Content-Length: 0` for methods that define request
    /// content semantics. Methods without those semantics intentionally remain
    /// unchanged.
    fn generate_empty_request_framing(op: &OperationInfo) -> TokenStream {
        if ["POST", "PUT", "PATCH"]
            .iter()
            .any(|method| op.method.eq_ignore_ascii_case(method))
        {
            quote! {
                req = req.header(reqwest::header::CONTENT_LENGTH, "0");
            }
        } else {
            quote! {}
        }
    }

    /// Find the success (2xx) response schema name, if any.
    ///
    /// Only considers 2xx status codes. Error schemas (4xx, 5xx) are ignored
    /// so that endpoints like 204 No Content correctly return `()` instead of
    /// accidentally picking up the error schema (e.g. `BadRequestError`).
    fn get_success_response_schema<'a>(
        &self,
        op: &'a OperationInfo,
    ) -> Option<(&'a str, &'a String)> {
        op.response_schemas
            .get_key_value("200")
            .or_else(|| op.response_schemas.get_key_value("201"))
            .or_else(|| {
                op.response_schemas
                    .iter()
                    .find(|(code, _)| code.starts_with('2'))
            })
            .map(|(status, schema)| (status.as_str(), schema))
    }

    fn get_success_response<'a>(
        &self,
        analysis: &'a SchemaAnalysis,
        op: &'a OperationInfo,
    ) -> ClientSuccessSelection<'a> {
        if let Some(responses) = analysis.operation_responses.get(&op.operation_id) {
            let mut candidates = Vec::new();
            for preferred in ["200", "201"] {
                if let Some((status, response)) = responses.get_key_value(preferred) {
                    candidates.push((status.as_str(), response));
                }
            }
            candidates.extend(
                responses
                    .iter()
                    .filter(|(status, _)| {
                        status.starts_with('2')
                            && status.as_str() != "200"
                            && status.as_str() != "201"
                    })
                    .map(|(status, response)| (status.as_str(), response)),
            );

            let selected = candidates
                .iter()
                .copied()
                .find(|(_, response)| {
                    matches!(
                        Self::response_body(response),
                        ClientSuccessBody::Json(_)
                            | ClientSuccessBody::Text
                            | ClientSuccessBody::Binary
                    )
                })
                .or_else(|| {
                    candidates.iter().copied().find(|(_, response)| {
                        matches!(
                            Self::response_body(response),
                            ClientSuccessBody::EventStream
                        )
                    })
                })
                .or_else(|| candidates.first().copied());

            if let Some((_, response)) = selected {
                let body = Self::response_body(response);
                let statuses: Vec<&str> = candidates
                    .iter()
                    .filter_map(|(status, candidate)| {
                        Self::success_bodies_are_compatible(body, Self::response_body(candidate))
                            .then_some(*status)
                    })
                    .collect();
                let accept = match &response.body {
                    Some(OperationResponseBody::Json { media_type, .. })
                    | Some(OperationResponseBody::Text { media_type }) => Some(media_type.as_str()),
                    Some(OperationResponseBody::Binary {
                        media_type,
                        wildcard,
                    }) => (!wildcard).then_some(media_type.as_str()),
                    None if response.schema_name.is_some() => {
                        response.media_type.as_deref().or(Some("application/json"))
                    }
                    None if response.supports_streaming => Some("text/event-stream"),
                    None => None,
                };
                let excluded_statuses = responses
                    .keys()
                    .filter(|status| {
                        status.len() == 3
                            && status.bytes().all(|c| c.is_ascii_digit())
                            && !statuses.contains(&status.as_str())
                    })
                    .map(String::as_str)
                    .collect();
                return ClientSuccessSelection {
                    statuses,
                    excluded_statuses,
                    body,
                    accept,
                };
            }
        }

        if let Some((_status, schema_name)) = self.get_success_response_schema(op) {
            let statuses = op
                .response_schemas
                .iter()
                .filter_map(|(candidate_status, candidate_schema)| {
                    (candidate_status.starts_with('2') && candidate_schema == schema_name)
                        .then_some(candidate_status.as_str())
                })
                .collect();
            ClientSuccessSelection {
                statuses,
                excluded_statuses: Vec::new(),
                body: ClientSuccessBody::Json(schema_name),
                accept: Some("application/json"),
            }
        } else if Self::returns_raw_event_stream(op) {
            ClientSuccessSelection {
                statuses: Vec::new(),
                excluded_statuses: Vec::new(),
                body: ClientSuccessBody::EventStream,
                accept: Some("text/event-stream"),
            }
        } else {
            ClientSuccessSelection {
                statuses: Vec::new(),
                excluded_statuses: Vec::new(),
                body: ClientSuccessBody::Empty,
                accept: None,
            }
        }
    }

    fn response_body(response: &crate::analysis::OperationResponse) -> ClientSuccessBody<'_> {
        match &response.body {
            Some(OperationResponseBody::Json { schema_name, .. }) => {
                ClientSuccessBody::Json(schema_name)
            }
            Some(OperationResponseBody::Text { .. }) => ClientSuccessBody::Text,
            Some(OperationResponseBody::Binary { .. }) => ClientSuccessBody::Binary,
            None if response.schema_name.is_some() => {
                ClientSuccessBody::Json(response.schema_name.as_deref().unwrap_or_default())
            }
            None if response.supports_streaming => ClientSuccessBody::EventStream,
            None => ClientSuccessBody::Empty,
        }
    }

    fn success_bodies_are_compatible(
        selected: ClientSuccessBody<'_>,
        candidate: ClientSuccessBody<'_>,
    ) -> bool {
        match (selected, candidate) {
            (ClientSuccessBody::Json(selected), ClientSuccessBody::Json(candidate)) => {
                selected == candidate
            }
            (ClientSuccessBody::Text, ClientSuccessBody::Text)
            | (ClientSuccessBody::Binary, ClientSuccessBody::Binary)
            | (ClientSuccessBody::EventStream, ClientSuccessBody::EventStream)
            | (ClientSuccessBody::Empty, ClientSuccessBody::Empty) => true,
            _ => false,
        }
    }

    fn response_type_for_body(&self, body: ClientSuccessBody<'_>) -> TokenStream {
        match body {
            ClientSuccessBody::Json(response_type) => {
                // Convert schema name to Rust type name (handles underscores, etc.)
                let rust_type_name = self.to_rust_type_name(response_type);
                let response_ident =
                    syn::Ident::new(&rust_type_name, proc_macro2::Span::call_site());
                quote! { #response_ident }
            }
            ClientSuccessBody::Text => quote! { String },
            ClientSuccessBody::Binary => quote! { bytes::Bytes },
            ClientSuccessBody::EventStream => {
                quote! { impl futures_util::Stream<Item = Result<bytes::Bytes, reqwest::Error>> }
            }
            ClientSuccessBody::ParsedSse => {
                quote! { super::sse::BoxSseStream<Result<super::sse::SseEvent<String>, super::sse::StreamingError>> }
            }
            ClientSuccessBody::Empty => quote! { () },
        }
    }

    fn success_status_guard(statuses: &[&str]) -> TokenStream {
        if statuses.is_empty() {
            return quote! { status.is_success() };
        }
        let guards = statuses
            .iter()
            .map(|status| Self::single_status_guard(status));
        quote! { false #( || #guards )* }
    }

    fn success_selection_guard(
        &self,
        _operation: &OperationInfo,
        success: &ClientSuccessSelection<'_>,
    ) -> TokenStream {
        let guard = Self::success_status_guard(&success.statuses);
        if !success
            .statuses
            .iter()
            .any(|status| status.eq_ignore_ascii_case("2XX"))
        {
            return guard;
        }
        let exact: Vec<_> = success
            .excluded_statuses
            .iter()
            .map(|status| Self::single_status_guard(status))
            .collect();
        quote! { (#guard) && !(false #( || #exact )*) }
    }

    fn single_status_guard(status: &str) -> TokenStream {
        match status {
            status if status.chars().all(|character| character.is_ascii_digit()) => {
                let status: u16 = status.parse().unwrap_or_default();
                quote! { status_code == #status }
            }
            status if matches!(status.as_bytes(), [b'1'..=b'5', b'X' | b'x', b'X' | b'x']) => {
                let class = u16::from(status.as_bytes()[0] - b'0');
                quote! { status_code / 100 == #class }
            }
            _ => quote! { status.is_success() },
        }
    }

    /// True when the operation's success response carries `text/event-stream`
    /// and nothing this generator can model as a JSON body.
    ///
    /// These previously generated `-> Result<(), _>` and then called
    /// `response.text().await`, which on a live SSE stream never returns: the
    /// caller's task deadlocks rather than erroring (openapi-generator-x9v).
    /// The streaming signal was already detected in analysis and honored by the
    /// server generator; only the client ignored it.
    ///
    /// Operations declaring *both* a JSON body and `text/event-stream` keep
    /// their JSON contract here — those are the `stream: true` style endpoints
    /// covered by the explicit `[streaming]` configuration, and silently
    /// changing their return type would break existing callers.
    fn returns_raw_event_stream(op: &OperationInfo) -> bool {
        op.supports_streaming
    }

    /// Generate error handling.
    ///
    /// Buffers non-streaming responses once, retaining both exact bytes and a
    /// lossy UTF-8 view for compatibility. Only the selected declared success
    /// status is parsed into the generated return type; other 2xx statuses are
    /// inspectable `ApiError`s rather than being fed to an incompatible parser.
    fn generate_error_handling(
        &self,
        op: &OperationInfo,
        success: ClientSuccessSelection<'_>,
    ) -> TokenStream {
        let op_error_type = self.op_error_type_token(op);
        let success_body = success.body;
        let success_status_guard = self.success_selection_guard(op, &success);
        let selected_status = if success.statuses.is_empty() {
            "any declared 2xx response".to_string()
        } else {
            success.statuses.join(", ")
        };

        let success_branch = match success_body {
            ClientSuccessBody::Json(_) => quote! {
                match serde_json::from_str(&body_text) {
                    Ok(body) => Ok(body),
                    Err(e) => Err(ApiOpError::Api(ApiError {
                        status: status_code,
                        headers: headers,
                        body: body_text,
                        raw_body,
                        typed: None,
                        parse_error: Some(format!(
                            "failed to deserialize 2xx response body: {}",
                            e
                        )),
                    })),
                }
            },
            ClientSuccessBody::Text => quote! {
                let _ = raw_body;
                Ok(body_text)
            },
            ClientSuccessBody::Empty => quote! {
                let _ = body_text;
                let _ = raw_body;
                let _ = headers;
                Ok(())
            },
            ClientSuccessBody::Binary
            | ClientSuccessBody::EventStream
            | ClientSuccessBody::ParsedSse => quote! {},
        };

        let error_match_arms = self.generate_error_match_arms(op);

        // Streaming success path: hand back the live byte stream instead of
        // buffering it. Reading an SSE body to a string blocks until the server
        // closes the connection, which is precisely what it will not do.
        // The error path still buffers — an error response is finite.
        if matches!(
            success_body,
            ClientSuccessBody::EventStream | ClientSuccessBody::ParsedSse
        ) {
            let stream = if matches!(success_body, ClientSuccessBody::ParsedSse) {
                quote! { super::sse::parse_sse_response(response) }
            } else {
                quote! { response.bytes_stream() }
            };
            return quote! {
                let status = response.status();
                let status_code = status.as_u16();
                let headers = response.headers().clone();

                if #success_status_guard {
                    Ok(#stream)
                } else {
                    if status.is_success() {
                        return Err(ApiOpError::Api(ApiError {
                            status: status_code,
                            headers,
                            body: String::new(),
                            raw_body: Vec::new(),
                            typed: None,
                            parse_error: Some(format!(
                                "unexpected successful status {}; generated return type selects `{}`; live response body was not buffered",
                                status_code,
                                #selected_status,
                            )),
                        }));
                    }
                    let body_bytes = __read_bounded_response_body(
                        response,
                        self.max_response_body_bytes,
                    ).await?;
                    let raw_body = body_bytes;
                    let body_text = String::from_utf8_lossy(&raw_body).into_owned();
                    let typed: Option<#op_error_type>;
                    let parse_error: Option<String>;
                    #error_match_arms
                    Err(ApiOpError::Api(ApiError {
                        status: status_code,
                        headers,
                        body: body_text,
                        raw_body,
                        typed,
                        parse_error,
                    }))
                }
            };
        }

        if matches!(success_body, ClientSuccessBody::Binary) {
            return quote! {
                let status = response.status();
                let status_code = status.as_u16();
                let headers = response.headers().clone();

                let body_bytes = __read_bounded_response_body(
                    response,
                    self.max_response_body_bytes,
                ).await?;
                if #success_status_guard {
                    Ok(bytes::Bytes::from(body_bytes))
                } else {
                    let raw_body = body_bytes;
                    let body_text = String::from_utf8_lossy(&raw_body).into_owned();
                    if status.is_success() {
                        return Err(ApiOpError::Api(ApiError {
                            status: status_code,
                            headers,
                            body: body_text,
                            raw_body,
                            typed: None,
                            parse_error: Some(format!(
                                "unexpected successful status {}; generated return type selects `{}`",
                                status_code,
                                #selected_status,
                            )),
                        }));
                    }
                    let typed: Option<#op_error_type>;
                    let parse_error: Option<String>;
                    #error_match_arms
                    Err(ApiOpError::Api(ApiError {
                        status: status_code,
                        headers,
                        body: body_text,
                        raw_body,
                        typed,
                        parse_error,
                    }))
                }
            };
        }

        quote! {
            let status = response.status();
            let status_code = status.as_u16();
            let headers = response.headers().clone();
            let body_bytes = __read_bounded_response_body(
                response,
                self.max_response_body_bytes,
            ).await?;
            let raw_body = body_bytes;
            let body_text = String::from_utf8_lossy(&raw_body).into_owned();

            if #success_status_guard {
                #success_branch
            } else if status.is_success() {
                Err(ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
                    raw_body,
                    typed: None,
                    parse_error: Some(format!(
                        "unexpected successful status {}; generated return type selects `{}`",
                        status_code,
                        #selected_status,
                    )),
                }))
            } else {
                let typed: Option<#op_error_type>;
                let parse_error: Option<String>;
                #error_match_arms
                Err(ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
                    raw_body,
                    typed,
                    parse_error,
                }))
            }
        }
    }

    /// Generate the match arms that select which per-op error variant to
    /// deserialize the response body into based on the runtime status code.
    fn generate_error_match_arms(&self, op: &OperationInfo) -> TokenStream {
        let arms: Vec<TokenStream> = op
            .response_schemas
            .iter()
            .filter(|(code, _)| !code.starts_with('2'))
            .filter_map(|(code, schema)| {
                let variant_ident = Self::op_error_variant_ident(code);
                let payload_ty_name = self.to_rust_type_name(schema);
                let payload_ty = syn::Ident::new(&payload_ty_name, proc_macro2::Span::call_site());
                let enum_ident = self.op_error_enum_ident(op);

                // T8: range-keyed responses (1XX/2XX/3XX/4XX/5XX) per OAS
                // 3.x §"Responses Object". Specific codes still take priority
                // (handled by ordering — concrete codes deserialize first
                // because the generic dispatch is a generic `_ if (range)`).
                let pattern = match code.as_str() {
                    "default" | "Default" => return None, // handled in fallback
                    other if other.chars().all(|c| c.is_ascii_digit()) => {
                        let n: u16 = other.parse().ok()?;
                        quote! { #n }
                    }
                    "1XX" | "1xx" => quote! { code if (100..=199).contains(&code) },
                    "2XX" | "2xx" => quote! { code if (200..=299).contains(&code) },
                    "3XX" | "3xx" => quote! { code if (300..=399).contains(&code) },
                    "4XX" | "4xx" => quote! { code if (400..=499).contains(&code) },
                    "5XX" | "5xx" => quote! { code if (500..=599).contains(&code) },
                    _ => return None,
                };

                Some(quote! {
                    #pattern => {
                        match serde_json::from_str::<#payload_ty>(&body_text) {
                            Ok(v) => {
                                typed = Some(#enum_ident::#variant_ident(v));
                                parse_error = None;
                            }
                            Err(e) => {
                                typed = None;
                                parse_error = Some(e.to_string());
                            }
                        }
                    }
                })
            })
            .collect();

        // Fallback for "default" or undeclared status codes: try to parse
        // as `serde_json::Value` for inspectability when the op's error
        // type is generic, otherwise leave typed = None.
        // Must mirror op_error_type_token: if op_error_type is the typed
        // enum (any non-2xx response, including `default`), the fallback arm
        // can't deserialize into `serde_json::Value` because `typed` is the
        // enum. Default to `typed = None` in that case.
        let has_typed_enum = op
            .response_schemas
            .iter()
            .any(|(code, _)| !code.starts_with('2'));

        // A spec-declared `default` response is the catch-all arm's payload
        // type. Without this the generated enum carries a `Default(..)` variant
        // that nothing ever constructs, so a response matched only by `default`
        // — a perfectly parseable typed body — still surfaces as
        // `typed: None` and callers fall back to raw strings
        // (openapi-generator-nu7).
        let default_payload = op
            .response_schemas
            .iter()
            .find(|(code, _)| matches!(code.as_str(), "default" | "Default"))
            .map(|(_, schema)| self.to_rust_type_name(schema));

        let default_arm = if let Some(payload_ty_name) = default_payload {
            let payload_ty = syn::Ident::new(&payload_ty_name, proc_macro2::Span::call_site());
            let enum_ident = self.op_error_enum_ident(op);
            quote! {
                _ => {
                    match serde_json::from_str::<#payload_ty>(&body_text) {
                        Ok(v) => {
                            typed = Some(#enum_ident::Default(v));
                            parse_error = None;
                        }
                        Err(e) => {
                            typed = None;
                            parse_error = Some(e.to_string());
                        }
                    }
                }
            }
        } else if has_typed_enum {
            quote! {
                _ => {
                    typed = None;
                    parse_error = None;
                }
            }
        } else {
            // No typed enum — op_error_type is serde_json::Value.
            quote! {
                _ => {
                    match serde_json::from_str::<serde_json::Value>(&body_text) {
                        Ok(v) => {
                            typed = Some(v);
                            parse_error = None;
                        }
                        Err(e) => {
                            typed = None;
                            parse_error = Some(e.to_string());
                        }
                    }
                }
            }
        };

        if arms.is_empty() {
            // No declared status arms — just the fallback.
            quote! {
                match status_code {
                    #default_arm
                }
            }
        } else {
            quote! {
                match status_code {
                    #(#arms)*
                    #default_arm
                }
            }
        }
    }

    /// Generate URL construction with path parameter substitution
    fn generate_url_construction(&self, path: &str, op: &OperationInfo) -> TokenStream {
        // Check if path has parameters (contains {...})
        if path.contains('{') {
            self.generate_url_with_params(path, op)
        } else {
            quote! {
                let request_url = format!("{}{}", self.base_url, #path);
            }
        }
    }

    /// Generate URL with path parameters
    fn generate_url_with_params(&self, path: &str, op: &OperationInfo) -> TokenStream {
        // Find all path parameters in the operation.
        let path_params: Vec<_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "path")
            .collect();

        // T5: percent-encode each path-template variable per RFC3986 §3.3.
        // We build a positional-arg format string by walking the template
        // left-to-right and emitting one `{}` + one format arg per
        // placeholder occurrence. Cloudflare has paths like
        // `/accounts/{account_id}/.../accounts/{account_id}` — the same
        // variable appears twice. A naive `replace_all` produced two `{}`
        // placeholders but only one format arg (E0277). Per-occurrence
        // emission keeps them in sync.
        let mut format_string = String::with_capacity(path.len());
        let mut format_args: Vec<TokenStream> = Vec::new();
        let mut chars = path.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '{' {
                format_string.push(c);
                continue;
            }
            // Read until the matching '}'.
            let mut name = String::new();
            while let Some(&n) = chars.peek() {
                chars.next();
                if n == '}' {
                    break;
                }
                name.push(n);
            }
            // Resolve to a path param. If no match, leave the placeholder
            // verbatim (real-world spec bug — this op shouldn't have made
            // it past analysis).
            let param = path_params.iter().find(|p| p.name == name);
            let Some(param) = param else {
                format_string.push('{');
                format_string.push_str(&name);
                format_string.push('}');
                continue;
            };
            format_string.push_str("{}");
            let param_name_snake = self.operation_param_ident(op, param);
            let param_ident = Self::to_field_ident(&param_name_snake);
            if Self::param_uses_as_ref_str(param) {
                format_args.push(quote! {
                    __pct_encode_path_segment(#param_ident.as_ref())
                });
            } else {
                format_args.push(quote! {
                    __pct_encode_path_segment(&#param_ident.to_string())
                });
            }
        }

        if format_args.is_empty() {
            quote! {
                let request_url = format!("{}{}", self.base_url, #path);
            }
        } else {
            quote! {
                let request_url = format!("{}{}", self.base_url, format!(#format_string, #(#format_args),*));
            }
        }
    }

    /// Resolve the Rust ident for a parameter. Prefers the disambiguated
    /// `rust_ident` set by the analyzer (which dedupes across the whole
    /// operation), falling back to a fresh sanitize of the wire name when
    /// no analyzer-side ident is present.
    pub(crate) fn param_ident_str(&self, param: &crate::analysis::ParameterInfo) -> String {
        if let Some(ident) = &param.rust_ident {
            // Apply the keyword-escape and self/super/crate dance the
            // sanitize fn does. The analyzer's base ident is already the
            // snake/kebab-aware shape; we only need post-processing.
            return self.escape_keyword_ident(ident);
        }
        self.sanitize_param_name(&param.name)
    }

    fn escape_keyword_ident(&self, snake_case: &str) -> String {
        if matches!(snake_case, "self" | "super" | "crate" | "Self") {
            return format!("{snake_case}_param");
        }
        if Self::is_rust_keyword(snake_case) {
            format!("r#{snake_case}")
        } else {
            snake_case.to_string()
        }
    }

    /// Sanitize a parameter name by escaping Rust reserved keywords with raw
    /// identifiers and disambiguating Twilio-style suffix operators
    /// (`StartTime`, `StartTime<`, `StartTime>` would otherwise all snake-
    /// case to `start_time`).
    fn sanitize_param_name(&self, name: &str) -> String {
        // Disambiguate before stripping. `<`, `>`, `<=`, `>=` are common in
        // filter-style query params; map them to `_lt` / `_gt` etc. so the
        // Rust ident is unique while the wire-level param name stays the
        // original string elsewhere in the codegen.
        let suffix = if name.ends_with("<=") {
            "_lte"
        } else if name.ends_with(">=") {
            "_gte"
        } else if name.ends_with('<') {
            "_lt"
        } else if name.ends_with('>') {
            "_gt"
        } else {
            ""
        };
        let stripped = name.trim_end_matches(['<', '>', '=']);
        let mut snake_case = stripped.to_snake_case();
        if snake_case.is_empty() {
            snake_case.push_str("parameter");
        } else if snake_case.starts_with(|character: char| character.is_ascii_digit()) {
            snake_case.insert(0, '_');
        }
        snake_case.push_str(suffix);

        if matches!(snake_case.as_str(), "self" | "super" | "crate" | "Self") {
            return format!("{snake_case}_param");
        }
        if Self::is_rust_keyword(&snake_case) {
            format!("r#{snake_case}")
        } else {
            snake_case
        }
    }
}
