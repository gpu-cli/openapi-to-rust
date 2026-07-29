//! Operation registry generation for OpenAPI specifications.
//!
//! Generates a static registry of operation metadata from analyzed OpenAPI operations.
//! The registry contains everything needed to:
//! - Build CLI subcommands dynamically (names, params, help text)
//! - Route and validate operations in a proxy (URL templates, param types, methods)
//!
//! The generated code is pure data — no HTTP client, no clap derives, no runtime dependencies
//! beyond serde. Consumers (CLI shims, proxy routers) interpret the registry generically.

use crate::analysis::SchemaAnalysis;
use crate::generator::CodeGenerator;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;

impl CodeGenerator {
    /// Generate the registry.rs file content
    pub fn generate_registry(&self, analysis: &SchemaAnalysis) -> crate::Result<String> {
        let provenance_attribute = self.provenance_attribute();
        let custom_methods = registry_custom_methods(analysis);
        let registry_types = Self::generate_registry_types(&custom_methods);
        let operation_defs = self.generate_operation_defs(analysis, &custom_methods);

        let tokens = quote! {
            //! Auto-generated operation registry. Do not edit.

            #provenance_attribute

            #registry_types
            #operation_defs
        };

        let file = syn::parse2(tokens).map_err(|e| {
            crate::GeneratorError::CodeGenError(format!("Failed to parse registry tokens: {}", e))
        })?;
        Ok(prettyplease::unparse(&file))
    }

    /// Generate the registry data types
    fn generate_registry_types(custom_methods: &BTreeMap<String, syn::Ident>) -> TokenStream {
        let custom_variants = custom_methods.values();
        let custom_as_str = custom_methods.iter().map(|(method, variant)| {
            quote! { Self::#variant => #method }
        });
        quote! {
            /// HTTP method for an operation
            #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
            pub enum HttpMethod {
                Get,
                Post,
                Put,
                Patch,
                Delete,
                Head,
                Options,
                Trace,
                Connect,
                Query,
                #(#custom_variants),*
            }

            impl HttpMethod {
                pub fn as_str(&self) -> &'static str {
                    match self {
                        Self::Get => "GET",
                        Self::Post => "POST",
                        Self::Put => "PUT",
                        Self::Patch => "PATCH",
                        Self::Delete => "DELETE",
                        Self::Head => "HEAD",
                        Self::Options => "OPTIONS",
                        Self::Trace => "TRACE",
                        Self::Connect => "CONNECT",
                        Self::Query => "QUERY",
                        #(#custom_as_str),*
                    }
                }
            }

            impl std::fmt::Display for HttpMethod {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str(self.as_str())
                }
            }

            /// Where a parameter appears in the request
            #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
            pub enum ParamLocation {
                Path,
                Query,
                Header,
            }

            /// Primitive type of a parameter (for validation and CLI parsing)
            #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
            pub enum ParamType {
                String,
                Integer,
                Number,
                Boolean,
            }

            /// Content type for request bodies
            #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
            pub enum BodyContentType {
                Json,
                FormUrlEncoded,
                Multipart,
                OctetStream,
                TextPlain,
            }

            /// Definition of an operation parameter.
            ///
            /// Only `Serialize` is derived: this struct holds `&'static`
            /// references to data baked into the binary, which cannot be
            /// reconstructed by `Deserialize`.
            #[derive(Debug, Clone, serde::Serialize)]
            pub struct ParamDef {
                pub name: &'static str,
                pub location: ParamLocation,
                pub required: bool,
                pub param_type: ParamType,
                pub description: Option<&'static str>,
            }

            /// Definition of a request body.
            ///
            /// `Serialize`-only for the same reason as [`ParamDef`].
            #[derive(Debug, Clone, serde::Serialize)]
            pub struct BodyDef {
                pub content_type: BodyContentType,
                /// Name of the schema type (for JSON/form bodies)
                pub schema_name: Option<&'static str>,
            }

            /// A single operation in the registry.
            ///
            /// `Serialize`-only because of the `&'static` fields.
            #[derive(Debug, Clone, serde::Serialize)]
            pub struct OperationDef {
                /// Unique operation identifier (e.g. "repos/get", "issues/create-comment")
                pub id: &'static str,
                /// HTTP method
                pub method: HttpMethod,
                /// URL path template with {param} placeholders
                pub path: &'static str,
                /// Short summary for CLI help
                pub summary: Option<&'static str>,
                /// Longer description
                pub description: Option<&'static str>,
                /// Parameters (path, query, header)
                pub params: &'static [ParamDef],
                /// Request body definition
                pub body: Option<BodyDef>,
                /// Response schema name for the success (2xx) case
                pub response_schema: Option<&'static str>,
            }

            /// Look up an operation by ID
            pub fn find_operation(id: &str) -> Option<&'static OperationDef> {
                OPERATIONS.iter().find(|op| op.id == id)
            }

            /// List all operation IDs
            pub fn operation_ids() -> impl Iterator<Item = &'static str> {
                OPERATIONS.iter().map(|op| op.id)
            }
        }
    }

    /// Generate the static OPERATIONS slice from analyzed operations
    fn generate_operation_defs(
        &self,
        analysis: &SchemaAnalysis,
        custom_methods: &BTreeMap<String, syn::Ident>,
    ) -> TokenStream {
        let mut param_statics: Vec<TokenStream> = Vec::new();
        let mut op_entries: Vec<TokenStream> = Vec::new();

        // Sort for deterministic output
        let mut sorted_ops: Vec<_> = analysis.operations.values().collect();
        sorted_ops.sort_by_key(|op| &op.operation_id);

        for op in sorted_ops {
            let id = &op.operation_id;
            let method = match op.method.to_uppercase().as_str() {
                "GET" => quote! { HttpMethod::Get },
                "POST" => quote! { HttpMethod::Post },
                "PUT" => quote! { HttpMethod::Put },
                "PATCH" => quote! { HttpMethod::Patch },
                "DELETE" => quote! { HttpMethod::Delete },
                "HEAD" => quote! { HttpMethod::Head },
                "OPTIONS" => quote! { HttpMethod::Options },
                "TRACE" => quote! { HttpMethod::Trace },
                "CONNECT" => quote! { HttpMethod::Connect },
                "QUERY" => quote! { HttpMethod::Query },
                other => {
                    let variant = &custom_methods[other];
                    quote! { HttpMethod::#variant }
                }
            };
            let path = &op.path;

            let summary = match &op.summary {
                Some(s) => quote! { Some(#s) },
                None => quote! { None },
            };
            let description = match &op.description {
                Some(d) => quote! { Some(#d) },
                None => quote! { None },
            };

            // Generate params
            let param_defs: Vec<TokenStream> = op
                .parameters
                .iter()
                .map(|p| {
                    let name = &p.name;
                    let location = match p.location.as_str() {
                        "path" => quote! { ParamLocation::Path },
                        "query" => quote! { ParamLocation::Query },
                        "header" => quote! { ParamLocation::Header },
                        _ => quote! { ParamLocation::Query },
                    };
                    let required = p.required;
                    let param_type = match p.rust_type.as_str() {
                        "i64" | "i32" => quote! { ParamType::Integer },
                        "f64" => quote! { ParamType::Number },
                        "bool" => quote! { ParamType::Boolean },
                        _ => quote! { ParamType::String },
                    };
                    let desc = match &p.description {
                        Some(d) => quote! { Some(#d) },
                        None => quote! { None },
                    };
                    quote! {
                        ParamDef {
                            name: #name,
                            location: #location,
                            required: #required,
                            param_type: #param_type,
                            description: #desc,
                        }
                    }
                })
                .collect();

            // Sanitize operation ID to a valid Rust identifier for the static name
            let sanitized_id: String = op
                .operation_id
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_uppercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            let params_static_name = syn::Ident::new(
                &format!("PARAMS_{sanitized_id}"),
                proc_macro2::Span::call_site(),
            );
            let param_count = param_defs.len();

            // Emit the param array as a separate static
            param_statics.push(quote! {
                static #params_static_name: [ParamDef; #param_count] = [#(#param_defs),*];
            });

            // Generate body def
            let body = match &op.request_body {
                Some(rb) => {
                    use crate::analysis::RequestBodyContent;
                    let (content_type, schema_name) = match rb {
                        RequestBodyContent::Json { schema_name, .. } => (
                            quote! { BodyContentType::Json },
                            quote! { Some(#schema_name) },
                        ),
                        RequestBodyContent::FormUrlEncoded { schema_name, .. } => (
                            quote! { BodyContentType::FormUrlEncoded },
                            quote! { Some(#schema_name) },
                        ),
                        RequestBodyContent::Multipart { schema_name, .. } => (
                            quote! { BodyContentType::Multipart },
                            quote! { Some(#schema_name) },
                        ),
                        RequestBodyContent::OctetStream { .. } => {
                            (quote! { BodyContentType::OctetStream }, quote! { None })
                        }
                        RequestBodyContent::Binary { .. } => {
                            (quote! { BodyContentType::OctetStream }, quote! { None })
                        }
                        RequestBodyContent::TextPlain { .. } => {
                            (quote! { BodyContentType::TextPlain }, quote! { None })
                        }
                        RequestBodyContent::Unsupported { .. } => {
                            (quote! { BodyContentType::OctetStream }, quote! { None })
                        }
                        RequestBodyContent::SchemaLess { .. } => continue,
                    };
                    quote! {
                        Some(BodyDef {
                            content_type: #content_type,
                            schema_name: #schema_name,
                        })
                    }
                }
                None => quote! { None },
            };

            // Response schema (2xx)
            let response_schema = op
                .response_schemas
                .get("200")
                .or_else(|| op.response_schemas.get("201"))
                .or_else(|| {
                    op.response_schemas
                        .iter()
                        .find(|(code, _)| code.starts_with('2'))
                        .map(|(_, v)| v)
                });
            let response_schema_token = match response_schema {
                Some(s) => quote! { Some(#s) },
                None => quote! { None },
            };

            op_entries.push(quote! {
                OperationDef {
                    id: #id,
                    method: #method,
                    path: #path,
                    summary: #summary,
                    description: #description,
                    params: &#params_static_name,
                    body: #body,
                    response_schema: #response_schema_token,
                }
            });
        }

        let op_count = op_entries.len();
        quote! {
            #(#param_statics)*

            pub static OPERATIONS: [OperationDef; #op_count] = [#(#op_entries),*];
        }
    }
}

/// Generate deterministic per-spec enum variants for extension methods. The
/// registry remains static data and keeps `Deserialize` support without
/// reducing arbitrary methods to a lossy catch-all variant.
fn registry_custom_methods(analysis: &SchemaAnalysis) -> BTreeMap<String, syn::Ident> {
    const STANDARD: &[&str] = &[
        "CONNECT", "DELETE", "GET", "HEAD", "OPTIONS", "PATCH", "POST", "PUT", "QUERY", "TRACE",
    ];
    analysis
        .operations
        .values()
        .map(|operation| operation.method.to_ascii_uppercase())
        .filter(|method| !STANDARD.contains(&method.as_str()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(index, method)| (method, format_ident!("Custom{index}")))
        .collect()
}
