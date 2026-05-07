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

use crate::analysis::{OperationInfo, SchemaAnalysis};
use crate::generator::CodeGenerator;
use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::quote;

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

            /// HTTP client for making API requests
            #[derive(Clone)]
            pub struct HttpClient {
                base_url: String,
                api_key: Option<String>,
                http_client: ClientWithMiddleware,
                custom_headers: BTreeMap<String, String>,
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

        // Combine all parts
        quote! {
            #retry_config_struct
            #client_struct

            impl HttpClient {
                #constructor
                #builder_methods
            }

            #default_impl
        }
    }

    /// Generate the constructor method
    fn generate_constructor(&self, has_retry: bool, has_tracing: bool) -> TokenStream {
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
                        base_url: String::new(),
                        api_key: None,
                        http_client,
                        custom_headers: BTreeMap::new(),
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
                        base_url: String::new(),
                        api_key: None,
                        http_client,
                        custom_headers: BTreeMap::new(),
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
    /// generated method signatures can reference them.
    pub fn generate_operation_methods(&self, analysis: &SchemaAnalysis) -> TokenStream {
        let op_error_enums: Vec<TokenStream> = analysis
            .operations
            .values()
            .filter_map(|op| self.generate_op_error_enum(op))
            .collect();

        let methods: Vec<TokenStream> = analysis
            .operations
            .values()
            .map(|op| self.generate_single_operation_method(op))
            .collect();

        quote! {
            #(#op_error_enums)*

            impl HttpClient {
                #(#methods)*
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
    fn generate_single_operation_method(&self, op: &OperationInfo) -> TokenStream {
        let method_name = self.get_method_name(op);
        let http_method = self.get_http_method(op);
        let path = &op.path;
        let request_param = self.generate_request_param(op);
        let request_body = self.generate_request_body(op);
        let query_params = self.generate_query_params(op);
        let response_type = self.get_response_type(op);
        let has_response_body = self.get_success_response_schema(op).is_some();
        let op_error_type = self.op_error_type_token(op);
        let error_handling = self.generate_error_handling(op, has_response_body);
        let url_construction = self.generate_url_construction(path, op);
        let doc_comment = self.generate_operation_doc_comment(op);

        quote! {
            #doc_comment
            pub async fn #method_name(
                &self,
                #request_param
            ) -> Result<#response_type, ApiOpError<#op_error_type>> {
                #url_construction

                let mut req = self.http_client
                    .#http_method(request_url)
                    #request_body;

                #query_params

                // Add API key if configured
                if let Some(api_key) = &self.api_key {
                    req = req.bearer_auth(api_key);
                }

                // Add custom headers
                for (name, value) in &self.custom_headers {
                    req = req.header(name, value);
                }

                let response = req.send().await?;
                #error_handling
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

        for param in query_params {
            // Use snake_case for Rust variable name with keyword escaping
            let param_name_snake = self.sanitize_param_name(&param.name);
            let param_name = Self::to_field_ident(&param_name_snake);

            // Use the original parameter name from OpenAPI spec as the query string key
            let param_key = &param.name;

            if param.required {
                // Required parameters: always add
                if param.rust_type == "String" {
                    param_building.push(quote! {
                        query_params.push((#param_key, #param_name.as_ref().to_string()));
                    });
                } else {
                    param_building.push(quote! {
                        query_params.push((#param_key, #param_name.to_string()));
                    });
                }
            } else {
                // Optional parameters: add only if Some
                if param.rust_type == "String" {
                    param_building.push(quote! {
                        if let Some(v) = #param_name {
                            query_params.push((#param_key, v.as_ref().to_string()));
                        }
                    });
                } else {
                    param_building.push(quote! {
                        if let Some(v) = #param_name {
                            query_params.push((#param_key, v.to_string()));
                        }
                    });
                }
            }
        }

        quote! {
            // Add query parameters
            {
                let mut query_params: Vec<(&str, String)> = Vec::new();
                #(#param_building)*
                if !query_params.is_empty() {
                    req = req.query(&query_params);
                }
            }
        }
    }

    /// Generate documentation comment for the operation
    fn generate_operation_doc_comment(&self, op: &OperationInfo) -> TokenStream {
        let method = op.method.to_uppercase();
        let path = &op.path;
        let doc = format!("{} {}", method, path);

        quote! {
            #[doc = #doc]
        }
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

        syn::Ident::new(&name, proc_macro2::Span::call_site())
    }

    /// Get the HTTP method
    fn get_http_method(&self, op: &OperationInfo) -> syn::Ident {
        let method = match op.method.to_uppercase().as_str() {
            "GET" => "get",
            "POST" => "post",
            "PUT" => "put",
            "DELETE" => "delete",
            "PATCH" => "patch",
            _ => "get", // Default fallback
        };

        syn::Ident::new(method, proc_macro2::Span::call_site())
    }

    /// Generate request parameters including path parameters, query parameters, and request body
    fn generate_request_param(&self, op: &OperationInfo) -> TokenStream {
        let mut params = Vec::new();

        // Add path parameters
        for param in &op.parameters {
            if param.location == "path" {
                let param_name_snake = self.sanitize_param_name(&param.name);
                let param_name = Self::to_field_ident(&param_name_snake);
                let param_type = self.get_param_rust_type(param);
                params.push(quote! { #param_name: #param_type });
            }
        }

        // Add query parameters (all as Option<T>)
        for param in &op.parameters {
            if param.location == "query" {
                let param_name_snake = self.sanitize_param_name(&param.name);
                let param_name = Self::to_field_ident(&param_name_snake);
                let param_type = self.get_param_rust_type(param);

                // Query parameters should be Option unless explicitly required
                if param.required {
                    params.push(quote! { #param_name: #param_type });
                } else {
                    params.push(quote! { #param_name: Option<#param_type> });
                }
            }
        }

        // Add request body parameter based on content type
        if let Some(ref rb) = op.request_body {
            use crate::analysis::RequestBodyContent;
            match rb {
                RequestBodyContent::Json { schema_name }
                | RequestBodyContent::FormUrlEncoded { schema_name } => {
                    let rust_type_name = self.to_rust_type_name(schema_name);
                    let request_ident =
                        syn::Ident::new(&rust_type_name, proc_macro2::Span::call_site());
                    params.push(quote! { request: #request_ident });
                }
                RequestBodyContent::Multipart => {
                    params.push(quote! { form: reqwest::multipart::Form });
                }
                RequestBodyContent::OctetStream => {
                    params.push(quote! { body: Vec<u8> });
                }
                RequestBodyContent::TextPlain => {
                    params.push(quote! { body: String });
                }
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
        let type_str = &param.rust_type;
        match type_str.as_str() {
            "String" => quote! { impl AsRef<str> },
            "i64" => quote! { i64 },
            "i32" => quote! { i32 },
            "f64" => quote! { f64 },
            "bool" => quote! { bool },
            _ => {
                let type_ident = syn::Ident::new(type_str, proc_macro2::Span::call_site());
                quote! { #type_ident }
            }
        }
    }

    /// Generate request body serialization based on content type
    fn generate_request_body(&self, op: &OperationInfo) -> TokenStream {
        if let Some(ref rb) = op.request_body {
            use crate::analysis::RequestBodyContent;
            match rb {
                RequestBodyContent::Json { .. } => {
                    quote! {
                        .body(serde_json::to_vec(&request).map_err(HttpError::serialization_error)?)
                        .header("content-type", "application/json")
                    }
                }
                RequestBodyContent::FormUrlEncoded { .. } => {
                    quote! {
                        .body(serde_urlencoded::to_string(&request).map_err(HttpError::serialization_error)?)
                        .header("content-type", "application/x-www-form-urlencoded")
                    }
                }
                RequestBodyContent::Multipart => {
                    quote! {
                        .multipart(form)
                    }
                }
                RequestBodyContent::OctetStream => {
                    quote! {
                        .body(body)
                        .header("content-type", "application/octet-stream")
                    }
                }
                RequestBodyContent::TextPlain => {
                    quote! {
                        .body(body)
                        .header("content-type", "text/plain")
                    }
                }
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
    fn get_success_response_schema<'a>(&self, op: &'a OperationInfo) -> Option<&'a String> {
        op.response_schemas
            .get("200")
            .or_else(|| op.response_schemas.get("201"))
            .or_else(|| {
                op.response_schemas
                    .iter()
                    .find(|(code, _)| code.starts_with('2'))
                    .map(|(_, v)| v)
            })
    }

    /// Get response type
    fn get_response_type(&self, op: &OperationInfo) -> TokenStream {
        if let Some(response_type) = self.get_success_response_schema(op) {
            // Convert schema name to Rust type name (handles underscores, etc.)
            let rust_type_name = self.to_rust_type_name(response_type);
            let response_ident = syn::Ident::new(&rust_type_name, proc_macro2::Span::call_site());
            quote! { #response_ident }
        } else {
            quote! { () }
        }
    }

    /// Generate error handling.
    ///
    /// Always reads the response body to a string before attempting any typed
    /// deserialization, so the raw body and headers are preserved on the error
    /// path even when JSON parsing fails. On 2xx the body is parsed into the
    /// success type; on non-2xx the body is parsed into the matching variant
    /// of the per-operation error enum (when one is declared) and wrapped in
    /// `ApiError<E>`.
    fn generate_error_handling(&self, op: &OperationInfo, has_response_body: bool) -> TokenStream {
        let op_error_type = self.op_error_type_token(op);

        let success_branch = if has_response_body {
            quote! {
                match serde_json::from_str(&body_text) {
                    Ok(body) => Ok(body),
                    Err(e) => Err(ApiOpError::Api(ApiError {
                        status: status_code,
                        headers: headers,
                        body: body_text,
                        typed: None,
                        parse_error: Some(format!(
                            "failed to deserialize 2xx response body: {}",
                            e
                        )),
                    })),
                }
            }
        } else {
            quote! {
                let _ = body_text;
                let _ = headers;
                Ok(())
            }
        };

        let error_match_arms = self.generate_error_match_arms(op);

        quote! {
            let status = response.status();
            let status_code = status.as_u16();
            let headers = response.headers().clone();
            let body_text = response.text().await
                .map_err(|e| ApiOpError::Transport(HttpError::Network(e)))?;

            if status.is_success() {
                #success_branch
            } else {
                let typed: Option<#op_error_type>;
                let parse_error: Option<String>;
                #error_match_arms
                Err(ApiOpError::Api(ApiError {
                    status: status_code,
                    headers,
                    body: body_text,
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

                let pattern = match code.as_str() {
                    "default" | "Default" => return None, // handled in fallback
                    other if other.chars().all(|c| c.is_ascii_digit()) => {
                        let n: u16 = other.parse().ok()?;
                        quote! { #n }
                    }
                    // Range like "4XX" / "5XX" — fall through to generic
                    // for now; declared-range handling is a follow-up.
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
        let has_typed_enum = op.response_schemas.iter().any(|(code, _)| {
            !code.starts_with('2') && !matches!(code.as_str(), "default" | "Default")
        });

        let default_arm = if has_typed_enum {
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
        // Parse path to find all parameter placeholders
        let mut format_string = path.to_string();
        let mut format_args = Vec::new();

        // Find all path parameters in the operation
        let path_params: Vec<_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "path")
            .collect();

        // Replace {paramName} with {} and collect parameter names for format args
        for param in &path_params {
            let placeholder = format!("{{{}}}", param.name);
            if format_string.contains(&placeholder) {
                format_string = format_string.replace(&placeholder, "{}");

                // Use snake_case for the Rust variable name with keyword escaping
                let param_name_snake = self.sanitize_param_name(&param.name);
                let param_ident = Self::to_field_ident(&param_name_snake);

                // Use .as_ref() for string types to handle impl AsRef<str>
                if param.rust_type == "String" {
                    format_args.push(quote! { #param_ident.as_ref() });
                } else {
                    format_args.push(quote! { #param_ident });
                }
            }
        }

        if format_args.is_empty() {
            // No path parameters found, use simple format
            quote! {
                let request_url = format!("{}{}", self.base_url, #path);
            }
        } else {
            // Build format call with path parameters
            quote! {
                let request_url = format!("{}{}", self.base_url, format!(#format_string, #(#format_args),*));
            }
        }
    }

    /// Sanitize a parameter name by escaping Rust reserved keywords with raw identifiers
    fn sanitize_param_name(&self, name: &str) -> String {
        let snake_case = name.to_snake_case();
        if Self::is_rust_keyword(&snake_case) {
            format!("r#{snake_case}")
        } else {
            snake_case
        }
    }
}
