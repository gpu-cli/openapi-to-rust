//! Server codegen — trait + typed response enums (P4).
//!
//! Emits one trait per tag (or a `ServerApi` trait for untagged
//! operations) plus a per-operation response enum with an
//! `IntoResponse` impl that maps each variant to its documented
//! status code.
//!
//! Router wiring, extractors, and SSE response variants are P5.

use crate::analysis::{
    ObjectAdditionalProperties, OperationInfo, RequestBodyContent, SchemaAnalysis, SchemaType,
};
use crate::config::ServerSection;
use crate::generator::{GeneratedFile, GeneratorConfig};

use super::{OperationIndex, Selector};
use heck::{ToPascalCase, ToSnakeCase};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Compute the set of schema names transitively reachable from the
/// request/response/parameter shapes of the given operations.
///
/// Used by client/server model pruning to drop unreferenced types from
/// `types.rs`. Walks every `$ref` in each schema's raw JSON
/// (`AnalyzedSchema.original`) rather than the analyzer's
/// `dependencies` field — the latter is incomplete for some
/// schemas (e.g. struct fields whose target schemas weren't
/// individually tracked).
///
/// Inline parameter enums (whose `rust_type` is a synthetic name
/// without a matching `analysis.schemas` entry) are not the
/// responsibility of this walk — they're emitted directly by the
/// server codegen from `parameter.enum_values`.
pub fn reachable_schemas(
    analysis: &SchemaAnalysis,
    ops: &[&OperationInfo],
) -> std::collections::BTreeSet<String> {
    reachable_schemas_with_roots(analysis, ops, &[])
}

/// [`reachable_schemas`] plus explicit schema roots used by configured
/// consumers such as SSE event-union types.
pub fn reachable_schemas_with_roots(
    analysis: &SchemaAnalysis,
    ops: &[&OperationInfo],
    extra_roots: &[String],
) -> std::collections::BTreeSet<String> {
    let mut keep: std::collections::BTreeSet<String> = Default::default();
    let mut queue: Vec<String> = Vec::new();

    let seed =
        |name: &str, queue: &mut Vec<String>, keep: &mut std::collections::BTreeSet<String>| {
            if !name.is_empty() && keep.insert(name.to_string()) {
                queue.push(name.to_string());
            }
        };

    for op in ops {
        if let Some(rb) = &op.request_body
            && let Some(name) = rb.schema_name()
        {
            seed(name, &mut queue, &mut keep);
        }
        for ty in op.response_schemas.values() {
            seed(ty, &mut queue, &mut keep);
        }
        for p in &op.parameters {
            if let Some(name) = &p.schema_ref {
                seed(name, &mut queue, &mut keep);
            }
        }
    }
    for root in extra_roots {
        seed(root, &mut queue, &mut keep);
    }

    while let Some(name) = queue.pop() {
        if let Some(schema) = analysis.schemas.get(&name) {
            // Walk the raw JSON for every `$ref` string and feed
            // the referenced schema names back into the queue.
            collect_refs(&schema.original, &mut queue, &mut keep);
            // Belt-and-braces: also include the analyzer's tracked
            // dependencies, which sometimes catch refs that live
            // outside the immediate JSON tree (e.g. allOf compositions
            // resolved before the snapshot was captured).
            for dep in &schema.dependencies {
                seed(dep, &mut queue, &mut keep);
            }
            // The analyzed shape is the authoritative generated type graph.
            // It includes ownership edges for inline/synthetic schemas that
            // do not appear as `$ref`s in the source document.
            collect_schema_type_refs(&schema.schema_type, &mut queue, &mut keep);
        }
    }

    keep
}

fn collect_schema_type_refs(
    schema_type: &SchemaType,
    queue: &mut Vec<String>,
    keep: &mut std::collections::BTreeSet<String>,
) {
    let seed =
        |name: &str, queue: &mut Vec<String>, keep: &mut std::collections::BTreeSet<String>| {
            if !name.is_empty() && keep.insert(name.to_string()) {
                queue.push(name.to_string());
            }
        };

    match schema_type {
        SchemaType::Primitive { .. }
        | SchemaType::StringEnum { .. }
        | SchemaType::ExtensibleEnum { .. } => {}
        SchemaType::Object {
            properties,
            additional_properties,
            ..
        } => {
            for property in properties.values() {
                collect_schema_type_refs(&property.schema_type, queue, keep);
            }
            if let ObjectAdditionalProperties::Typed { value_type } = additional_properties {
                collect_schema_type_refs(value_type, queue, keep);
            }
        }
        SchemaType::DiscriminatedUnion { variants, .. } => {
            for variant in variants {
                seed(&variant.type_name, queue, keep);
            }
        }
        SchemaType::Union { variants } | SchemaType::Composition { schemas: variants } => {
            for variant in variants {
                seed(&variant.target, queue, keep);
            }
        }
        SchemaType::Array { item_type } => collect_schema_type_refs(item_type, queue, keep),
        SchemaType::Reference { target } => seed(target, queue, keep),
    }
}

fn collect_refs(
    value: &serde_json::Value,
    queue: &mut Vec<String>,
    keep: &mut std::collections::BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                if k == "$ref"
                    && let Some(s) = v.as_str()
                    && let Some(name) = s.strip_prefix("#/components/schemas/")
                    && keep.insert(name.to_string())
                {
                    queue.push(name.to_string());
                }
                collect_refs(v, queue, keep);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                collect_refs(v, queue, keep);
            }
        }
        _ => {}
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServerCodegenError {
    #[error("server selector: {0}")]
    Parse(#[from] super::SelectorParseError),
    #[error("server selector: {0}")]
    Resolve(#[from] super::SelectorResolveError),
    #[error("internal: {0}")]
    Internal(String),
    #[error(
        "cannot generate exact Axum routes for custom HTTP methods on `{path}` across multiple primary tags ({tags}); Axum 0.7 cannot merge multiple fallback dispatchers for one path. Put those operations under the same first tag or select only one custom method for this server"
    )]
    CrossTagCustomMethods { path: String, tags: String },
}

pub struct ServerCodegen<'a> {
    config: &'a GeneratorConfig,
    analysis: &'a SchemaAnalysis,
    server: &'a ServerSection,
}

impl<'a> ServerCodegen<'a> {
    pub fn new(
        config: &'a GeneratorConfig,
        analysis: &'a SchemaAnalysis,
        server: &'a ServerSection,
    ) -> Self {
        Self {
            config,
            analysis,
            server,
        }
    }

    /// Resolve selectors and emit `server/{mod,api,errors}.rs`.
    pub fn generate(&self) -> Result<Vec<GeneratedFile>, ServerCodegenError> {
        if self.server.operations.is_empty() {
            return Ok(Vec::new());
        }

        let index = OperationIndex::from_analysis(self.analysis);
        let selectors: Vec<Selector> = self
            .server
            .operations
            .iter()
            .map(|s| Selector::parse(s))
            .collect::<Result<_, _>>()?;
        let resolution = super::resolve(&selectors, &index)?;

        // Look up full OperationInfo for each resolved op (we need
        // parameters, request body, response schemas — the summary
        // only has the display surface).
        let ops: Vec<&OperationInfo> = resolution
            .operations
            .iter()
            .map(|s| {
                self.analysis
                    .operations
                    .get(&s.operation_id)
                    .ok_or_else(|| {
                        ServerCodegenError::Internal(format!(
                            "operation `{}` resolved but missing from analysis",
                            s.operation_id
                        ))
                    })
            })
            .collect::<Result<_, _>>()?;
        validate_custom_method_route_groups(&ops)?;

        // Group by primary tag (first tag wins; untagged → "Server").
        let groups = group_by_tag(&ops);

        let api_rs = self.emit_api(&groups);
        let errors_rs = self.emit_errors(&ops);
        let router_rs = self.emit_router(&groups);
        let mod_rs = self.emit_mod();

        Ok(vec![
            GeneratedFile {
                path: PathBuf::from("server").join("mod.rs"),
                content: format_or_raw(mod_rs),
            },
            GeneratedFile {
                path: PathBuf::from("server").join("api.rs"),
                content: format_or_raw(api_rs),
            },
            GeneratedFile {
                path: PathBuf::from("server").join("errors.rs"),
                content: format_or_raw(errors_rs),
            },
            GeneratedFile {
                path: PathBuf::from("server").join("router.rs"),
                content: format_or_raw(router_rs),
            },
        ])
    }

    fn emit_mod(&self) -> TokenStream {
        let _ = &self.config; // keep field access future-proof
        quote! {
            //! Server scaffolding emitted by openapi-to-rust.
            //!
            //! Implement the per-tag trait(s) in `api` on your own struct,
            //! then build an `axum::Router` via `router::router(impl)`.

            pub mod api;
            pub mod errors;
            pub mod router;

            pub use api::*;
            pub use errors::*;
            pub use router::*;
        }
    }

    fn emit_router(&self, groups: &BTreeMap<String, Vec<&OperationInfo>>) -> TokenStream {
        let factories: Vec<TokenStream> = groups
            .iter()
            .map(|(tag, ops)| self.emit_router_for_trait(tag, ops))
            .collect();

        // Per-op Query structs — one per op that has any query params.
        let query_structs: Vec<TokenStream> = groups
            .values()
            .flatten()
            .filter_map(|op| self.emit_query_struct(op))
            .collect();

        // When the picked operations span multiple tags, emit a
        // top-level `build_router(impl1, impl2, ...)` that takes one
        // generic per trait and `.merge()`s the per-tag factories.
        // For a single-tag selection this is unnecessary noise — the
        // user calls the per-tag factory directly.
        let combined = if groups.len() > 1 {
            Some(self.emit_combined_router(groups))
        } else {
            None
        };

        quote! {
            //! Router factories — one per trait. Each takes any
            //! `T: <TraitName> + Clone + Send + Sync + 'static` and
            //! returns an `axum::Router` with state pre-attached.

            use super::api::*;
            use super::errors::*;
            // Pull schemas directly from the types module (always a
            // sibling of mod.rs). Doesn't rely on the parent module
            // re-exporting types::*, so users can mount the generated
            // tree at any path without rewriting these imports.
            #[allow(unused_imports)]
            use super::super::types::*;

            #(#query_structs)*

            #(#factories)*

            #combined
        }
    }

    fn emit_combined_router(&self, groups: &BTreeMap<String, Vec<&OperationInfo>>) -> TokenStream {
        // Stable ordering: BTreeMap iteration is already alphabetical
        // by tag, which gives us deterministic generic ordering across
        // generator runs.
        let entries: Vec<(syn::Ident, syn::Ident, syn::Ident)> = groups
            .keys()
            .enumerate()
            .map(|(i, tag)| {
                let trait_ident = trait_ident_for_tag(tag);
                let factory = format_ident!("{}_router", trait_ident.to_string().to_snake_case());
                let generic = format_ident!("T{}", i + 1);
                (trait_ident, factory, generic)
            })
            .collect();

        let generics: Vec<&syn::Ident> = entries.iter().map(|(_, _, g)| g).collect();
        let args: Vec<TokenStream> = entries
            .iter()
            .map(|(trait_ident, _, g)| {
                let arg_ident = format_ident!("{}", trait_ident.to_string().to_snake_case());
                quote! { #arg_ident: #g }
            })
            .collect();
        let bounds: Vec<TokenStream> = entries
            .iter()
            .map(|(trait_ident, _, g)| {
                quote! { #g: #trait_ident + Clone + Send + Sync + 'static }
            })
            .collect();

        // Fold the factories: `factory1(arg1).merge(factory2(arg2)).merge(...)`.
        let first = &entries[0];
        let first_arg = format_ident!("{}", first.0.to_string().to_snake_case());
        let first_factory = &first.1;
        let rest = entries
            .iter()
            .skip(1)
            .map(|(trait_ident, factory, _)| {
                let arg = format_ident!("{}", trait_ident.to_string().to_snake_case());
                quote! { .merge(#factory(#arg)) }
            })
            .collect::<Vec<_>>();

        let trait_names: Vec<String> = entries.iter().map(|(t, _, _)| t.to_string()).collect();
        let doc = format!(
            " Combined router spanning {} traits: {}.",
            entries.len(),
            trait_names.join(", "),
        );

        quote! {
            #[doc = #doc]
            pub fn build_router<#(#generics),*>(
                #(#args),*
            ) -> ::axum::Router
            where
                #(#bounds),*
            {
                #first_factory(#first_arg) #(#rest)*
            }
        }
    }

    fn emit_router_for_trait(&self, tag: &str, ops: &[&OperationInfo]) -> TokenStream {
        let trait_ident = trait_ident_for_tag(tag);
        let fn_ident = format_ident!("{}_router", trait_ident.to_string().to_snake_case());

        let mut routes: Vec<TokenStream> = Vec::new();
        let mut custom_by_path: BTreeMap<String, Vec<(String, syn::Ident)>> = BTreeMap::new();
        for op in ops {
            let handler = format_ident!("{}_handler", op.operation_id.to_snake_case());
            let path = openapi_to_axum_path(&op.path);
            if let Some(method_call) = axum_method_call(&op.method) {
                routes.push(quote! { .route(#path, ::axum::routing::#method_call(#handler::<T>)) });
            } else {
                custom_by_path
                    .entry(path)
                    .or_default()
                    .push((op.method.to_ascii_uppercase(), handler));
            }
        }
        let mut custom_dispatchers = Vec::new();
        for (path, methods) in custom_by_path {
            let first_handler = &methods[0].1;
            let dispatcher = format_ident!("{}_custom_method_dispatch", first_handler);
            let (route, dispatcher_fn) =
                axum_custom_route(&path, &dispatcher, &methods, &trait_ident);
            routes.push(route);
            custom_dispatchers.push(dispatcher_fn);
        }

        let handlers: Vec<TokenStream> = ops
            .iter()
            .map(|op| self.emit_axum_handler(&trait_ident, op))
            .collect();

        let doc = format!(" Build an axum::Router for the `{trait_ident}` trait.");

        quote! {
            #[doc = #doc]
            pub fn #fn_ident<T>(api: T) -> ::axum::Router
            where
                T: #trait_ident + Clone + Send + Sync + 'static,
            {
                ::axum::Router::new()
                    #(#routes)*
                    .with_state(api)
            }

            #(#custom_dispatchers)*

            #(#handlers)*
        }
    }

    fn emit_axum_handler(&self, trait_ident: &syn::Ident, op: &OperationInfo) -> TokenStream {
        let handler_ident = format_ident!("{}_handler", op.operation_id.to_snake_case());
        let trait_method = format_ident!("{}", op.operation_id.to_snake_case());

        // Build extractor list + call argument list.
        let mut extractors: Vec<TokenStream> =
            vec![quote! { ::axum::extract::State(api): ::axum::extract::State<T> }];
        let mut call_args: Vec<TokenStream> = Vec::new();

        // Path parameters → axum::extract::Path tuple
        let path_params: Vec<&_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "path")
            .collect();
        if !path_params.is_empty() {
            let idents: Vec<syn::Ident> = path_params
                .iter()
                .map(|p| format_ident!("{}", p.name.to_snake_case()))
                .collect();
            let types: Vec<TokenStream> = path_params
                .iter()
                .map(|p| parse_type(&p.rust_type))
                .collect();
            if path_params.len() == 1 {
                let i = &idents[0];
                let t = &types[0];
                extractors.push(quote! { ::axum::extract::Path(#i): ::axum::extract::Path<#t> });
            } else {
                extractors.push(quote! { ::axum::extract::Path((#(#idents),*)): ::axum::extract::Path<(#(#types),*)> });
            }
            for i in &idents {
                call_args.push(quote! { #i });
            }
        }

        // Query parameters — extract via a per-op `<Op>Query` struct
        // (emitted in the same router.rs above). Required params are
        // unwrapped here (short-circuit 400 if missing) so the trait
        // method sees a `T` rather than `Option<T>`.
        let query_params: Vec<&_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "query")
            .collect();
        let mut required_query_checks: Vec<TokenStream> = Vec::new();
        if !query_params.is_empty() {
            let query_ident = format_ident!("{}Query", op.operation_id.to_pascal_case());
            extractors.push(quote! {
                ::axum::extract::Query(__q): ::axum::extract::Query<#query_ident>
            });
            for p in &query_params {
                let f = format_ident!("{}", p.name.to_snake_case());
                let wire = p.name.as_str();
                if p.required {
                    let missing_msg = format!("missing required query parameter `{wire}`");
                    required_query_checks.push(quote! {
                        let #f = match __q.#f {
                            Some(v) => v,
                            None => return ::axum::response::IntoResponse::into_response(
                                (
                                    ::axum::http::StatusCode::BAD_REQUEST,
                                    ::axum::Json(::serde_json::json!({
                                        "error": #missing_msg
                                    })),
                                )
                            ),
                        };
                    });
                    call_args.push(quote! { #f });
                } else {
                    call_args.push(quote! { __q.#f });
                }
            }
        }

        // Header parameters — extract via HeaderMap and read each
        // header by name. Required headers short-circuit with 400 if
        // missing or non-UTF-8.
        let header_params: Vec<&_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "header")
            .collect();
        let mut required_header_checks: Vec<TokenStream> = Vec::new();
        if !header_params.is_empty() {
            extractors.push(quote! { __headers: ::axum::http::HeaderMap });
            for p in &header_params {
                let wire = p.name.as_str();
                let ident = format_ident!("{}", header_param_ident(&p.name));
                if p.required {
                    let missing_msg = format!("missing required header `{wire}`");
                    required_header_checks.push(quote! {
                        let #ident = match __headers
                            .get(#wire)
                            .and_then(|v| v.to_str().ok())
                            .map(::std::string::String::from)
                        {
                            Some(v) => v,
                            None => return ::axum::response::IntoResponse::into_response(
                                (
                                    ::axum::http::StatusCode::BAD_REQUEST,
                                    ::axum::Json(::serde_json::json!({
                                        "error": #missing_msg
                                    })),
                                )
                            ),
                        };
                    });
                    call_args.push(quote! { #ident });
                } else {
                    call_args.push(quote! {
                        __headers
                            .get(#wire)
                            .and_then(|v| v.to_str().ok())
                            .map(::std::string::String::from)
                    });
                }
            }
        }

        // Body
        let body_ty_opt = body_type(op);
        if let Some(body_ty) = &body_ty_opt {
            let body_ty_tokens = parse_type(body_ty);
            if op.request_body_required {
                extractors.push(quote! {
                    ::axum::Json(body): ::axum::Json<#body_ty_tokens>
                });
                call_args.push(quote! { body });
            } else {
                extractors.push(quote! {
                    body: ::std::option::Option<::axum::Json<#body_ty_tokens>>
                });
                call_args.push(quote! { body.map(|::axum::Json(b)| b) });
            }
        }

        let _ = format_ident!("{}Response", op.operation_id.to_pascal_case());
        // Keep referencing trait_ident so the where-bound name is
        // visible to downstream readers — clippy would otherwise flag
        // it as unused in some configurations.
        let _ = trait_ident;

        // Handler returns `axum::response::Response` so the required-
        // param short-circuit (400 BadRequest) and the trait method's
        // typed response enum (via IntoResponse) can both flow out
        // through the same return type.
        quote! {
            async fn #handler_ident<T>(
                #(#extractors),*
            ) -> ::axum::response::Response
            where
                T: super::api::#trait_ident + Clone + Send + Sync + 'static,
            {
                #(#required_query_checks)*
                #(#required_header_checks)*
                ::axum::response::IntoResponse::into_response(
                    api.#trait_method(#(#call_args),*).await,
                )
            }
        }
    }

    fn emit_api(&self, groups: &BTreeMap<String, Vec<&OperationInfo>>) -> TokenStream {
        let _ = &self.config; // reserved for future module-aware emission
        let traits: Vec<TokenStream> = groups
            .iter()
            .map(|(tag, ops)| self.emit_trait(tag, ops))
            .collect();

        // Inline string enums declared on parameters get synthetic
        // type names (e.g. `ListInputItemsOrder`). The analyzer
        // surfaces enum_values; we emit the enum here so the trait
        // signature compiles. Dedup by name in case two ops in the
        // same picked set share the same synthetic name.
        let mut emitted: std::collections::BTreeSet<String> = Default::default();
        let mut param_enums: Vec<TokenStream> = Vec::new();
        for op in groups.values().flatten() {
            for p in &op.parameters {
                if let Some(values) = &p.enum_values {
                    if emitted.insert(p.rust_type.clone()) {
                        param_enums.push(emit_param_enum(&p.rust_type, values));
                    }
                }
            }
        }

        quote! {
            //! Per-tag traits. Implement one of these on your own
            //! struct; the router (P5) wires it into axum.

            #![allow(clippy::too_many_arguments)]

            use super::errors::*;
            // Schemas live in `<parent>/types.rs`. Reaching them via
            // `super::super::types::*` instead of a glob on the
            // parent module keeps these imports stable regardless of
            // how the user mounts the generated tree.
            #[allow(unused_imports)]
            use super::super::types::*;

            #(#param_enums)*

            #(#traits)*
        }
    }

    fn emit_trait(&self, tag: &str, ops: &[&OperationInfo]) -> TokenStream {
        let trait_ident = trait_ident_for_tag(tag);
        let methods: Vec<TokenStream> = ops.iter().map(|op| self.emit_method_sig(op)).collect();
        let doc = format!(" Operations under the `{tag}` tag.");
        quote! {
            #[doc = #doc]
            #[axum::async_trait]
            pub trait #trait_ident: Send + Sync + 'static {
                #(#methods)*
            }
        }
    }

    fn emit_method_sig(&self, op: &OperationInfo) -> TokenStream {
        let name = format_ident!("{}", op.operation_id.to_snake_case());
        let response_ty = format_ident!("{}Response", op.operation_id.to_pascal_case());

        // Order: path → query → header → body. Required params keep
        // their declared rust_type; optional params wrap in Option<…>.
        // This mirrors what the router handler extracts so positional
        // ordering matches the call site exactly.
        let mut params: Vec<TokenStream> = Vec::new();
        for p in &op.parameters {
            if p.location == "path" {
                let ident = format_ident!("{}", p.name.to_snake_case());
                let ty = parse_type(&p.rust_type);
                params.push(quote! { #ident: #ty });
            }
        }
        for p in &op.parameters {
            if p.location == "query" {
                let ident = format_ident!("{}", p.name.to_snake_case());
                let ty = parse_type(&p.rust_type);
                // Required query params land as `T`; the handler
                // validates presence and returns 400 if absent, so
                // by the time the trait method sees the value it
                // must be Some. Optional → `Option<T>`.
                if p.required {
                    params.push(quote! { #ident: #ty });
                } else {
                    params.push(quote! { #ident: ::std::option::Option<#ty> });
                }
            }
        }
        for p in &op.parameters {
            if p.location == "header" {
                let ident = format_ident!("{}", header_param_ident(&p.name));
                if p.required {
                    params.push(quote! { #ident: String });
                } else {
                    params.push(quote! { #ident: ::std::option::Option<String> });
                }
            }
        }
        if let Some(body) = body_type(op) {
            let body_ty = parse_type(&body);
            if op.request_body_required {
                params.push(quote! { body: #body_ty });
            } else {
                params.push(quote! { body: Option<#body_ty> });
            }
        }

        let summary_doc = op
            .summary
            .as_deref()
            .map(|s| format!(" {s}"))
            .unwrap_or_default();
        let route_doc = format!(" `{} {}`", op.method, op.path);

        quote! {
            #[doc = #summary_doc]
            #[doc = ""]
            #[doc = #route_doc]
            async fn #name(&self, #(#params),*) -> #response_ty;
        }
    }

    /// Per-op `<Op>Query` struct emitted into router.rs when the op
    /// has any query parameters. Drives axum's `Query<T>` extractor.
    fn emit_query_struct(&self, op: &OperationInfo) -> Option<TokenStream> {
        let query_params: Vec<&_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "query")
            .collect();
        if query_params.is_empty() {
            return None;
        }
        let ident = format_ident!("{}Query", op.operation_id.to_pascal_case());
        let fields: Vec<TokenStream> = query_params
            .iter()
            .map(|p| {
                let f_ident = format_ident!("{}", p.name.to_snake_case());
                let ty = parse_type(&p.rust_type);
                let serde_rename = if p.name.to_snake_case() == p.name {
                    quote! {}
                } else {
                    let wire = p.name.as_str();
                    quote! { #[serde(rename = #wire)] }
                };
                quote! {
                    #serde_rename
                    #[serde(default)]
                    pub #f_ident: ::std::option::Option<#ty>
                }
            })
            .collect();
        let doc = format!(
            " Query parameters for `{} {}` (operationId `{}`).",
            op.method, op.path, op.operation_id
        );
        Some(quote! {
            #[doc = #doc]
            #[derive(Debug, Default, ::serde::Deserialize)]
            pub struct #ident {
                #(#fields),*
            }
        })
    }

    fn emit_errors(&self, ops: &[&OperationInfo]) -> TokenStream {
        let _ = &self.config; // reserved for future module-aware emission
        let any_streaming = ops.iter().any(|op| op.supports_streaming);
        let enums: Vec<TokenStream> = ops.iter().map(|op| self.emit_response_enum(op)).collect();

        // The SSE type alias is emitted exactly when at least one
        // picked op streams. Bringing it in unconditionally would force
        // `futures-core` into the user's dep tree even when they don't
        // need it.
        let stream_alias = if any_streaming {
            quote! {
                /// Stream payload carried by `*Stream` variants. Each
                /// yielded item is a pre-built `axum::response::sse::Event`.
                pub type ServerEventStream = ::std::pin::Pin<
                    Box<
                        dyn ::futures_core::Stream<
                                Item = ::std::result::Result<
                                    ::axum::response::sse::Event,
                                    ::std::convert::Infallible,
                                >,
                            > + ::std::marker::Send
                            + 'static,
                    >,
                >;

                /// Wrap any `Stream<Item = Result<Event, Infallible>>` in
                /// a `Sse<ServerEventStream>` ready to drop into the
                /// `OkStream` variant. Replaces the
                /// `Sse::new(Box::pin(...))` dance.
                pub fn sse_response<S>(stream: S) -> ::axum::response::sse::Sse<ServerEventStream>
                where
                    S: ::futures_core::Stream<
                            Item = ::std::result::Result<
                                ::axum::response::sse::Event,
                                ::std::convert::Infallible,
                            >,
                        > + ::std::marker::Send
                        + 'static,
                {
                    ::axum::response::sse::Sse::new(Box::pin(stream))
                }
            }
        } else {
            quote! {}
        };

        // We deliberately do NOT import `axum::response::Response` here:
        // many specs declare a schema literally named `Response`
        // (OpenAI's `createResponse` is one such case), and an explicit
        // import would shadow the glob-imported schema name. The
        // IntoResponse impl returns `axum::response::Response`
        // fully qualified.
        quote! {
            //! Per-operation response enums. Pick a variant to pick a
            //! status code — IntoResponse maps each variant to its
            //! documented (StatusCode, Json) pair.

            #![allow(clippy::large_enum_variant)]

            use axum::{
                http::StatusCode,
                response::IntoResponse,
                Json,
            };
            // Schemas live in `<parent>/types.rs`. Reaching them via
            // `super::super::types::*` instead of a glob on the
            // parent module keeps these imports stable regardless of
            // how the user mounts the generated tree.
            #[allow(unused_imports)]
            use super::super::types::*;

            #stream_alias

            #(#enums)*
        }
    }

    fn emit_response_enum(&self, op: &OperationInfo) -> TokenStream {
        let enum_ident = format_ident!("{}Response", op.operation_id.to_pascal_case());
        let mut variants: Vec<TokenStream> = Vec::new();
        let mut arms: Vec<TokenStream> = Vec::new();

        for (status, schema_name) in &op.response_schemas {
            let variant = format_ident!("{}", status_variant_name(status));
            let body_ty = parse_type(schema_name);
            variants.push(quote! { #variant(#body_ty) });
            let status_expr = status_token(status);
            arms.push(quote! {
                Self::#variant(body) => (#status_expr, Json(body)).into_response()
            });
        }

        // SSE: when the operation declares text/event-stream on any
        // response, add a streaming sibling variant. The user picks
        // it when their request had `stream: true` (or whatever the
        // streaming trigger is). Variant payload is a fully built
        // `axum::Sse` so the user controls keep-alive, retry interval,
        // etc.
        if op.supports_streaming {
            variants.push(quote! {
                OkStream(::axum::response::sse::Sse<ServerEventStream>)
            });
            arms.push(quote! {
                Self::OkStream(sse) => sse.into_response()
            });
        }

        // Fallback: if no response variants were declared we still need
        // a no-op enum so the trait method has a return type. Use an
        // empty `Empty` variant returning 204.
        if variants.is_empty() {
            variants.push(quote! { Empty });
            arms.push(quote! {
                Self::Empty => StatusCode::NO_CONTENT.into_response()
            });
        }

        let doc = format!(
            " Response for `{} {}` (operationId `{}`).",
            op.method, op.path, op.operation_id
        );

        quote! {
            #[doc = #doc]
            pub enum #enum_ident {
                #(#variants),*
            }

            impl IntoResponse for #enum_ident {
                fn into_response(self) -> ::axum::response::Response {
                    match self {
                        #(#arms),*
                    }
                }
            }
        }
    }
}

/// Convert a wire-level header name (e.g. `anthropic-version`,
/// `X-Request-Id`) to a Rust identifier (`anthropic_version`,
/// `x_request_id`). Snake_case lowercases + replaces hyphens.
fn header_param_ident(name: &str) -> String {
    name.replace('-', "_").to_snake_case()
}

/// Emit a string-enum type for a parameter whose inline schema
/// declared `enum: [...]`. The analyzer sets `rust_type` to a
/// synthetic name (`{OpId}{Param}` in PascalCase) and surfaces the
/// values; the codegen layer is what actually writes the enum.
fn emit_param_enum(name: &str, values: &[String]) -> TokenStream {
    let enum_ident = format_ident!("{}", name);
    let variants: Vec<TokenStream> = values
        .iter()
        .enumerate()
        .map(|(i, raw)| {
            let pascal = raw.to_pascal_case();
            // PascalCase can produce an empty string (pure-symbol
            // input) or an identifier starting with a digit
            // (e.g. `1d` stays `1d`) — both invalid as Rust idents.
            // Fall back to a positional name so the enum compiles.
            let starts_with_digit = pascal
                .chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(true);
            let v_name = if pascal.is_empty() || starts_with_digit {
                format!("Variant{i}")
            } else {
                pascal
            };
            let v_ident = format_ident!("{}", v_name);
            let default_marker = if i == 0 {
                quote! { #[default] }
            } else {
                quote! {}
            };
            quote! {
                #default_marker
                #[serde(rename = #raw)]
                #v_ident
            }
        })
        .collect();
    quote! {
        #[derive(Debug, Clone, PartialEq, Eq, ::serde::Deserialize, ::serde::Serialize, Default)]
        pub enum #enum_ident {
            #(#variants),*
        }
    }
}

fn axum_method_call(method: &str) -> Option<TokenStream> {
    match method.to_ascii_uppercase().as_str() {
        "CONNECT" => Some(quote! { connect }),
        "DELETE" => Some(quote! { delete }),
        "GET" => Some(quote! { get }),
        "HEAD" => Some(quote! { head }),
        "OPTIONS" => Some(quote! { options }),
        "PATCH" => Some(quote! { patch }),
        "POST" => Some(quote! { post }),
        "PUT" => Some(quote! { put }),
        "TRACE" => Some(quote! { trace }),
        _ => None,
    }
}

/// Build one exact-method dispatcher for every nonstandard operation sharing a
/// path within one generated trait. Axum has convenience functions for the
/// standard RFC methods, but OpenAPI 3.2 also defines QUERY and permits custom
/// `additionalOperations`. Axum allows only one `any` fallback per path, so all
/// custom methods on that path and trait must share this dispatcher. Generation
/// rejects the cross-trait form before reaching this helper.
fn axum_custom_route(
    path: &str,
    dispatcher: &syn::Ident,
    methods: &[(String, syn::Ident)],
    trait_ident: &syn::Ident,
) -> (TokenStream, TokenStream) {
    let arms = methods.iter().map(|(method, handler)| {
        quote! {
            #method => ::axum::handler::Handler::call(#handler::<T>, request, api).await
        }
    });
    let route = quote! {
        .route(#path, ::axum::routing::any(#dispatcher::<T>))
    };
    let dispatcher_fn = quote! {
        async fn #dispatcher<T>(
            ::axum::extract::State(api): ::axum::extract::State<T>,
            request: ::axum::extract::Request,
        ) -> ::axum::response::Response
        where
            T: #trait_ident + Clone + Send + Sync + 'static,
        {
            match request.method().as_str() {
                #(#arms,)*
                _ => ::axum::response::IntoResponse::into_response(
                    ::axum::http::StatusCode::METHOD_NOT_ALLOWED,
                ),
            }
        }
    };
    (route, dispatcher_fn)
}

/// OpenAPI uses `{param}` placeholders; axum 0.7 accepts the same
/// `{param}` syntax for typed extraction, so this is currently a
/// pass-through. The helper exists so future syntax shifts (e.g.
/// nested wildcards) live in one place.
fn openapi_to_axum_path(p: &str) -> String {
    p.to_string()
}

fn body_type(op: &OperationInfo) -> Option<String> {
    match &op.request_body {
        Some(RequestBodyContent::Json { schema_name })
        | Some(RequestBodyContent::FormUrlEncoded { schema_name }) => Some(schema_name.clone()),
        _ => None,
    }
}

fn group_by_tag<'a>(ops: &[&'a OperationInfo]) -> BTreeMap<String, Vec<&'a OperationInfo>> {
    let mut groups: BTreeMap<String, Vec<&OperationInfo>> = BTreeMap::new();
    for op in ops {
        let tag = primary_tag(op);
        groups.entry(tag).or_default().push(op);
    }
    groups
}

fn primary_tag(op: &OperationInfo) -> String {
    op.tags.first().cloned().unwrap_or_else(|| "Server".into())
}

fn validate_custom_method_route_groups(ops: &[&OperationInfo]) -> Result<(), ServerCodegenError> {
    let mut tags_by_path: BTreeMap<&str, std::collections::BTreeSet<String>> = BTreeMap::new();
    for op in ops {
        if axum_method_call(&op.method).is_none() {
            tags_by_path
                .entry(&op.path)
                .or_default()
                .insert(primary_tag(op));
        }
    }
    if let Some((path, tags)) = tags_by_path.into_iter().find(|(_, tags)| tags.len() > 1) {
        return Err(ServerCodegenError::CrossTagCustomMethods {
            path: path.to_string(),
            tags: tags.into_iter().collect::<Vec<_>>().join(", "),
        });
    }
    Ok(())
}

fn trait_ident_for_tag(tag: &str) -> syn::Ident {
    let pascal = tag.to_pascal_case();
    let base = if pascal.is_empty() {
        "Server".into()
    } else {
        pascal
    };
    format_ident!("{}Api", base)
}

/// Convert a status code (or `default`, or wildcard `4XX`) to a
/// variant identifier.
fn status_variant_name(status: &str) -> String {
    match status {
        "200" => "Ok".into(),
        "201" => "Created".into(),
        "202" => "Accepted".into(),
        "204" => "NoContent".into(),
        "301" => "MovedPermanently".into(),
        "302" => "Found".into(),
        "304" => "NotModified".into(),
        "400" => "BadRequest".into(),
        "401" => "Unauthorized".into(),
        "403" => "Forbidden".into(),
        "404" => "NotFound".into(),
        "409" => "Conflict".into(),
        "410" => "Gone".into(),
        "422" => "UnprocessableEntity".into(),
        "429" => "TooManyRequests".into(),
        "500" => "InternalServerError".into(),
        "502" => "BadGateway".into(),
        "503" => "ServiceUnavailable".into(),
        "default" => "Default".into(),
        "2XX" => "Success".into(),
        "3XX" => "Redirection".into(),
        "4XX" => "ClientError".into(),
        "5XX" => "ServerError".into(),
        other => format!("Status{}", other.to_ascii_uppercase().replace('X', "x")),
    }
}

/// Emit a StatusCode expression for a status string. Numeric codes use
/// the named constants where possible; wildcard ranges and `default`
/// pick a representative code (the lowest in-range).
fn status_token(status: &str) -> TokenStream {
    match status {
        "200" => quote! { StatusCode::OK },
        "201" => quote! { StatusCode::CREATED },
        "202" => quote! { StatusCode::ACCEPTED },
        "204" => quote! { StatusCode::NO_CONTENT },
        "301" => quote! { StatusCode::MOVED_PERMANENTLY },
        "302" => quote! { StatusCode::FOUND },
        "304" => quote! { StatusCode::NOT_MODIFIED },
        "400" => quote! { StatusCode::BAD_REQUEST },
        "401" => quote! { StatusCode::UNAUTHORIZED },
        "403" => quote! { StatusCode::FORBIDDEN },
        "404" => quote! { StatusCode::NOT_FOUND },
        "409" => quote! { StatusCode::CONFLICT },
        "410" => quote! { StatusCode::GONE },
        "422" => quote! { StatusCode::UNPROCESSABLE_ENTITY },
        "429" => quote! { StatusCode::TOO_MANY_REQUESTS },
        "500" => quote! { StatusCode::INTERNAL_SERVER_ERROR },
        "502" => quote! { StatusCode::BAD_GATEWAY },
        "503" => quote! { StatusCode::SERVICE_UNAVAILABLE },
        "default" => quote! { StatusCode::INTERNAL_SERVER_ERROR },
        "2XX" => quote! { StatusCode::OK },
        "3XX" => quote! { StatusCode::MOVED_PERMANENTLY },
        "4XX" => quote! { StatusCode::BAD_REQUEST },
        "5XX" => quote! { StatusCode::INTERNAL_SERVER_ERROR },
        // Specific numeric codes not in our table — fall back to
        // StatusCode::from_u16. Codegen ensures a panic-free path by
        // unwrapping on a value that must parse (we already
        // know the spec wrote a numeric status here).
        other => {
            if let Ok(n) = other.parse::<u16>() {
                quote! {
                    StatusCode::from_u16(#n).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
                }
            } else {
                quote! { StatusCode::INTERNAL_SERVER_ERROR }
            }
        }
    }
}

fn parse_type(ty: &str) -> TokenStream {
    syn::parse_str::<syn::Type>(ty)
        .map(|t| quote! { #t })
        .unwrap_or_else(|_| {
            let ident = format_ident!("{}", ty);
            quote! { #ident }
        })
}

fn format_or_raw(ts: TokenStream) -> String {
    let raw = ts.to_string();
    match syn::parse_file(&raw) {
        Ok(parsed) => prettyplease::unparse(&parsed),
        Err(_) => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_variant_name_maps_known_codes() {
        assert_eq!(status_variant_name("200"), "Ok");
        assert_eq!(status_variant_name("4XX"), "ClientError");
        assert_eq!(status_variant_name("default"), "Default");
        assert_eq!(status_variant_name("418"), "Status418");
    }

    #[test]
    fn trait_ident_for_tag_appends_api() {
        let id = trait_ident_for_tag("Responses");
        assert_eq!(id.to_string(), "ResponsesApi");
    }

    #[test]
    fn untagged_falls_back_to_server_api() {
        let id = trait_ident_for_tag("");
        assert_eq!(id.to_string(), "ServerApi");
    }

    #[test]
    fn custom_methods_on_one_path_share_an_exact_dispatcher() {
        let dispatcher = format_ident!("cache_custom_method_dispatch");
        let methods = vec![
            ("PURGE".to_string(), format_ident!("purge_cache_handler")),
            ("QUERY".to_string(), format_ident!("query_cache_handler")),
        ];
        let trait_ident = format_ident!("CacheApi");
        let (route, dispatcher) = axum_custom_route("/cache", &dispatcher, &methods, &trait_ident);
        let route = route.to_string();
        let dispatcher = dispatcher.to_string();
        assert!(route.contains("routing :: any"));
        assert_eq!(route.matches("routing :: any").count(), 1);
        assert!(dispatcher.contains("\"PURGE\""));
        assert!(dispatcher.contains("\"QUERY\""));
        assert!(dispatcher.contains("purge_cache_handler"));
        assert!(dispatcher.contains("query_cache_handler"));
        assert!(dispatcher.contains("METHOD_NOT_ALLOWED"));
    }

    #[test]
    fn standard_methods_use_axum_method_routes_without_guards() {
        assert!(axum_method_call("TRACE").is_some());
        assert!(axum_method_call("QUERY").is_none());
    }
}
