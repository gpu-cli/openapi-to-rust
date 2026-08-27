//! Server codegen — trait + typed response enums (P4).
//!
//! Emits one trait per tag (or a `ServerApi` trait for untagged
//! operations) plus a per-operation response enum with an
//! `IntoResponse` impl that maps each variant to its documented
//! status code.
//!
//! Router wiring, extractors, and SSE response variants are P5.

use crate::analysis::{
    ObjectAdditionalProperties, OperationInfo, OperationResponse, OperationResponseBody,
    ParameterInfo, QuerySerialization, RequestBodyContent, SchemaAnalysis, SchemaType,
};
use crate::config::ServerSection;
use crate::generator::{CodeGenerator, GeneratedFile, GeneratorConfig, rust_type_name};

use super::{OperationIndex, Selector};
use heck::{ToPascalCase, ToSnakeCase};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy)]
enum MultipartFieldKind {
    Binary,
    String,
    Integer,
    UnsignedInteger,
    Number,
    Boolean,
}

struct MultipartFieldPlan {
    wire_name: String,
    field_ident: syn::Ident,
    required: bool,
    kind: MultipartFieldKind,
}

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
            if let Some(
                QuerySerialization::FormExplodedArray {
                    item_type: crate::analysis::ArrayItemType::SchemaRef(name),
                }
                | QuerySerialization::FormArray {
                    item_type: crate::analysis::ArrayItemType::SchemaRef(name),
                }
                | QuerySerialization::SimpleHeaderArray {
                    item_type: crate::analysis::ArrayItemType::SchemaRef(name),
                },
            ) = &p.query_serialization
            {
                seed(name, &mut queue, &mut keep);
            }
            if let Some(
                QuerySerialization::FormExplodedArray {
                    item_type:
                        crate::analysis::ArrayItemType::FlatStructRef { schema_name, .. }
                        | crate::analysis::ArrayItemType::NestedStructRef { schema_name, .. },
                }
                | QuerySerialization::FormArray {
                    item_type:
                        crate::analysis::ArrayItemType::FlatStructRef { schema_name, .. }
                        | crate::analysis::ArrayItemType::NestedStructRef { schema_name, .. },
                }
                | QuerySerialization::SimpleHeaderArray {
                    item_type:
                        crate::analysis::ArrayItemType::FlatStructRef { schema_name, .. }
                        | crate::analysis::ArrayItemType::NestedStructRef { schema_name, .. },
                },
            ) = &p.query_serialization
            {
                seed(schema_name, &mut queue, &mut keep);
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
        SchemaType::Tuple { element_types } => {
            for element_type in element_types {
                collect_schema_type_refs(element_type, queue, keep);
            }
        }
        SchemaType::Reference { target } => seed(target, queue, keep),
        SchemaType::Untyped { .. } => {}
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
        "cannot generate exact Axum routes for custom HTTP methods on `{path}` across multiple primary tags ({tags}); Axum cannot merge multiple fallback dispatchers for one path. Put those operations under the same first tag or select only one custom method for this server"
    )]
    CrossTagCustomMethods { path: String, tags: String },
    #[error(
        "cannot generate Axum server for distinct primary tags `{first_tag}` and `{second_tag}`: both normalize to Rust trait identifier `{identifier}` (and the same router factory name). Rename one tag in the OpenAPI document or a schema overlay, or select the operations in separate generated servers"
    )]
    TagIdentifierCollision {
        first_tag: String,
        second_tag: String,
        identifier: String,
    },
    #[error("cannot generate Axum route for `{path}`: {reason}")]
    InvalidRoutePath { path: String, reason: String },
    #[error(
        "cannot generate Axum query extraction for `{operation_id}` parameter `{parameter}`: {reason}"
    )]
    UnsupportedQueryParameter {
        operation_id: String,
        parameter: String,
        reason: String,
    },
    #[error(
        "cannot generate unambiguous Axum query extraction for `{operation_id}`: wire key `{wire_key}` is claimed by both `{first_parameter}` and `{second_parameter}`"
    )]
    AmbiguousQueryParameter {
        operation_id: String,
        wire_key: String,
        first_parameter: String,
        second_parameter: String,
    },
    #[error("request validation: {0}")]
    Validation(String),
    #[error(
        "cannot generate Axum extraction for `{operation_id}` {location} parameter `{parameter}`: only scalar schema serialization is currently supported"
    )]
    UnsupportedParameterSerialization {
        operation_id: String,
        location: String,
        parameter: String,
    },
    #[error(
        "cannot generate Axum request body for `{operation_id}` media type `{media_type}`: {reason}"
    )]
    UnsupportedRequestBody {
        operation_id: String,
        media_type: String,
        reason: String,
    },
    #[error(
        "cannot generate response for `{operation_id}` status `{status}`: unsupported media content {media_types}"
    )]
    UnsupportedResponseContent {
        operation_id: String,
        status: String,
        media_types: String,
    },
}

pub struct ServerCodegen<'a> {
    config: &'a GeneratorConfig,
    analysis: &'a SchemaAnalysis,
    server: &'a ServerSection,
    source_provenance: Option<String>,
}

impl<'a> ServerCodegen<'a> {
    fn resolve_multipart_schema<'b>(
        &'b self,
        schema: &'b serde_json::Value,
    ) -> Result<&'b serde_json::Value, ServerCodegenError> {
        self.resolve_multipart_schema_inner(schema, &mut std::collections::HashSet::new())
    }

    fn resolve_multipart_schema_inner<'b>(
        &'b self,
        schema: &'b serde_json::Value,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<&'b serde_json::Value, ServerCodegenError> {
        let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) else {
            return Ok(schema);
        };
        let Some(name) = reference.strip_prefix("#/components/schemas/") else {
            return Ok(schema);
        };
        if !visited.insert(name.to_string()) {
            return Err(ServerCodegenError::Validation(format!(
                "multipart schema reference cycle includes `{name}`"
            )));
        }
        let resolved = self
            .analysis
            .validation_context
            .component_schemas
            .get(name)
            .ok_or_else(|| {
                ServerCodegenError::Validation(format!(
                    "multipart schema reference `{reference}` could not be resolved"
                ))
            })?;
        self.resolve_multipart_schema_inner(resolved, visited)
    }

    fn multipart_field_plans(
        &self,
        schema: &serde_json::Value,
    ) -> Result<Vec<MultipartFieldPlan>, ServerCodegenError> {
        let schema = self.resolve_multipart_schema(schema)?;
        let properties = schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                ServerCodegenError::Validation(
                    "multipart request schema must be a flat object with declared properties"
                        .into(),
                )
            })?;
        if schema
            .get("additionalProperties")
            .is_some_and(|value| value != &serde_json::Value::Bool(false))
        {
            return Err(ServerCodegenError::Validation(
                "multipart request schema cannot use additionalProperties".into(),
            ));
        }
        let required = schema
            .get("required")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect::<std::collections::HashSet<_>>();
        properties
            .iter()
            .map(|(wire_name, property)| {
                let property = self.resolve_multipart_schema(property)?;
                let kind = match (
                    property.get("type").and_then(serde_json::Value::as_str),
                    property.get("format").and_then(serde_json::Value::as_str),
                ) {
                    (Some("string"), Some("binary")) => match self.config.types.binary {
                        crate::type_mapping::BinaryStrategy::String => MultipartFieldKind::String,
                        crate::type_mapping::BinaryStrategy::Bytes
                        | crate::type_mapping::BinaryStrategy::VecU8 => MultipartFieldKind::Binary,
                    },
                    (Some("string"), _) => MultipartFieldKind::String,
                    (Some("integer"), Some("uint32" | "uint64" | "uint"))
                        if self.config.types.unsigned =>
                    {
                        MultipartFieldKind::UnsignedInteger
                    }
                    (Some("integer"), _) => MultipartFieldKind::Integer,
                    (Some("number"), _) => MultipartFieldKind::Number,
                    (Some("boolean"), _) => MultipartFieldKind::Boolean,
                    _ => {
                        return Err(ServerCodegenError::Validation(format!(
                            "multipart field `{wire_name}` must be binary or a scalar text field"
                        )));
                    }
                };
                Ok(MultipartFieldPlan {
                    wire_name: wire_name.clone(),
                    field_ident: CodeGenerator::to_field_ident(&wire_name.to_snake_case()),
                    required: required.contains(wire_name.as_str()),
                    kind,
                })
            })
            .collect()
    }

    pub fn new(
        config: &'a GeneratorConfig,
        analysis: &'a SchemaAnalysis,
        server: &'a ServerSection,
    ) -> Self {
        Self {
            config,
            analysis,
            server,
            source_provenance: None,
        }
    }

    /// Attach a sanitized source label to generated server module headers.
    pub fn with_source_provenance(mut self, source: Option<&str>) -> Self {
        self.source_provenance = source.map(str::to_string);
        self
    }

    fn provenance_attribute(&self) -> TokenStream {
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

    /// Resolve a model type reference for emitted server modules. Names that
    /// canonicalize to a different Rust identifier (e.g. `not-found`) cannot
    /// be parsed from the raw component key and must be qualified explicitly;
    /// already-canonical names resolve through the module's
    /// `use super::super::types::*` glob like every other generated reference.
    fn model_type(&self, ty: &str) -> TokenStream {
        if self.analysis.schemas.contains_key(ty) {
            let canonical = rust_type_name(ty);
            if canonical != ty {
                let ident = format_ident!("{}", canonical);
                return quote! { super::super::types::#ident };
            }
        }
        parse_type(ty)
    }

    /// Resolve selectors and emit `server/{mod,api,errors}.rs`.
    pub fn generate(&self) -> Result<Vec<GeneratedFile>, ServerCodegenError> {
        if self.server.operations.is_empty() {
            return Ok(Vec::new());
        }
        if !(1..=100).contains(&self.server.validation.max_errors) {
            return Err(ServerCodegenError::Validation(
                "max_errors must be between 1 and 100".to_string(),
            ));
        }
        if !(1..=67_108_864).contains(&self.server.validation.max_body_bytes) {
            return Err(ServerCodegenError::Validation(
                "max_body_bytes must be between 1 and 67108864".to_string(),
            ));
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
        validate_tag_identifier_collisions(&ops)?;
        validate_custom_method_route_groups(&ops)?;
        validate_normalized_route_collisions(&ops)?;
        self.validate_query_parameters(&ops)?;
        self.validate_supported_server_inputs(&ops)?;
        self.validate_supported_server_outputs(&ops)?;
        if !self.server.validation.enabled
            && ops.iter().any(|operation| {
                operation
                    .parameters
                    .iter()
                    .any(|parameter| matches!(parameter.location.as_str(), "header" | "cookie"))
                    || matches!(
                        &operation.request_body,
                        Some(RequestBodyContent::FormUrlEncoded { .. })
                    )
            })
        {
            return Err(ServerCodegenError::Validation(
                "typed header/cookie and form extraction requires server.validation.enabled=true"
                    .to_string(),
            ));
        }

        // Group by primary tag (first tag wins; untagged → "Server").
        let groups = group_by_tag(&ops);
        let response_enum_names = self.allocate_response_enum_names(&ops);
        let has_binary_body = ops.iter().any(|operation| {
            matches!(
                operation.request_body,
                Some(RequestBodyContent::OctetStream { .. } | RequestBodyContent::Binary { .. })
            )
        });
        let has_text_body = ops.iter().any(|operation| {
            matches!(
                operation.request_body,
                Some(RequestBodyContent::TextPlain { .. })
            )
        });
        let has_embedded_path_affixes = ops.iter().any(|operation| {
            operation.parameters.iter().any(|parameter| {
                parameter.location == "path"
                    && path_parameter_affixes(&operation.path, &parameter.name).is_some()
            })
        });

        let validation_bundle = if self.server.validation.enabled {
            Some(
                super::validation::prepare_validation_bundle(
                    &self.analysis.validation_context,
                    &ops,
                )
                .map_err(|error| ServerCodegenError::Validation(error.to_string()))?,
            )
        } else {
            eprintln!(
                "⚠️  server.validation.enabled=false: generated handlers will not enforce the OpenAPI request contract"
            );
            None
        };

        let transport_validation = has_binary_body || has_text_body;
        let validation_module_enabled =
            validation_bundle.is_some() || transport_validation || has_embedded_path_affixes;
        let api_rs = self.emit_api(&groups, &response_enum_names);
        let errors_rs = self.emit_errors(&ops, validation_module_enabled, &response_enum_names);
        let router_rs = self.emit_router(&groups, validation_bundle.as_ref())?;
        let mod_rs = self.emit_mod(validation_module_enabled);

        let mut files = vec![
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
        ];
        if let Some(bundle) = &validation_bundle {
            files.push(GeneratedFile {
                path: PathBuf::from("server").join("validation.rs"),
                content: format_or_raw(super::validation::emit_validation_module(
                    bundle,
                    self.server.validation.max_errors,
                    has_binary_body,
                    has_text_body,
                )),
            });
        } else if transport_validation || has_embedded_path_affixes {
            files.push(GeneratedFile {
                path: PathBuf::from("server").join("validation.rs"),
                content: format_or_raw(super::validation::emit_transport_validation_module(
                    has_binary_body,
                    has_text_body,
                )),
            });
        }
        Ok(files)
    }

    fn query_parameter_type(&self, parameter: &ParameterInfo) -> TokenStream {
        CodeGenerator::new(self.config.clone()).get_param_owned_rust_type(parameter)
    }

    fn parameter_schema_is_scalar(&self, schema: &serde_json::Value) -> bool {
        self.parameter_schema_is_scalar_inner(schema, &mut std::collections::BTreeSet::new())
    }

    fn parameter_schema_is_string(&self, parameter: &ParameterInfo) -> bool {
        if parameter.enum_values.is_some() || parameter.rust_type == "String" {
            return true;
        }
        let Some(schema) = parameter.validation_schema.as_ref() else {
            return false;
        };
        self.parameter_schema_is_string_inner(schema, &mut std::collections::BTreeSet::new())
    }

    fn parameter_schema_is_string_inner(
        &self,
        schema: &serde_json::Value,
        visited: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str) {
            if let Some(name) = reference.strip_prefix("#/components/schemas/") {
                return self
                    .analysis
                    .validation_context
                    .component_schemas
                    .get(name)
                    .is_some_and(|component| {
                        visited.insert(name.to_string())
                            && self.parameter_schema_is_string_inner(component, visited)
                    });
            }
        }
        schema.get("type").and_then(serde_json::Value::as_str) == Some("string")
    }

    fn parameter_schema_is_scalar_inner(
        &self,
        schema: &serde_json::Value,
        visited: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        if let Some(reference) = schema.get("$ref").and_then(serde_json::Value::as_str)
            && let Some(name) = reference.strip_prefix("#/components/schemas/")
            && let Some(component) = self.analysis.validation_context.component_schemas.get(name)
        {
            return visited.insert(name.to_string())
                && self.parameter_schema_is_scalar_inner(component, visited);
        }
        !matches!(
            schema.get("type").and_then(serde_json::Value::as_str),
            Some("array" | "object")
        ) && schema.get("oneOf").is_none()
            && schema.get("anyOf").is_none()
            && schema.get("allOf").is_none()
    }

    fn form_field_names(
        &self,
        operation: &OperationInfo,
    ) -> Result<Vec<String>, ServerCodegenError> {
        let Some(RequestBodyContent::FormUrlEncoded { schema_name, .. }) = &operation.request_body
        else {
            return Ok(Vec::new());
        };
        let schema = self.resolve_query_schema(schema_name).ok_or_else(|| {
            ServerCodegenError::UnsupportedRequestBody {
                operation_id: operation.operation_id.clone(),
                media_type: "application/x-www-form-urlencoded".to_string(),
                reason: format!("schema `{schema_name}` cannot be resolved"),
            }
        })?;
        let SchemaType::Object {
            properties,
            additional_properties,
            ..
        } = &schema.schema_type
        else {
            return Err(ServerCodegenError::UnsupportedRequestBody {
                operation_id: operation.operation_id.clone(),
                media_type: "application/x-www-form-urlencoded".to_string(),
                reason: "only flat object schemas are supported".to_string(),
            });
        };
        if !matches!(additional_properties, ObjectAdditionalProperties::Forbidden)
            || properties.values().any(|property| {
                !self.query_property_is_scalar(
                    &property.schema_type,
                    &mut std::collections::HashSet::new(),
                )
            })
        {
            return Err(ServerCodegenError::UnsupportedRequestBody {
                operation_id: operation.operation_id.clone(),
                media_type: "application/x-www-form-urlencoded".to_string(),
                reason: "only flat scalar fields with additionalProperties forbidden are supported"
                    .to_string(),
            });
        }
        Ok(properties.keys().cloned().collect())
    }

    fn validate_supported_server_inputs(
        &self,
        operations: &[&OperationInfo],
    ) -> Result<(), ServerCodegenError> {
        for operation in operations {
            for parameter in &operation.parameters {
                if !matches!(
                    parameter.location.as_str(),
                    "path" | "query" | "header" | "cookie"
                ) {
                    return Err(ServerCodegenError::UnsupportedParameterSerialization {
                        operation_id: operation.operation_id.clone(),
                        location: parameter.location.clone(),
                        parameter: parameter.name.clone(),
                    });
                }
                if matches!(parameter.location.as_str(), "path" | "header" | "cookie")
                    && !matches!(
                        parameter.query_serialization,
                        Some(QuerySerialization::SimpleHeaderArray { .. })
                    )
                    && parameter
                        .validation_schema
                        .as_ref()
                        .is_none_or(|schema| !self.parameter_schema_is_scalar(schema))
                {
                    return Err(ServerCodegenError::UnsupportedParameterSerialization {
                        operation_id: operation.operation_id.clone(),
                        location: parameter.location.clone(),
                        parameter: parameter.name.clone(),
                    });
                }
            }
            match &operation.request_body {
                Some(RequestBodyContent::FormUrlEncoded { .. }) => {
                    self.form_field_names(operation)?;
                }
                Some(RequestBodyContent::Multipart {
                    validation_schema, ..
                }) => {
                    self.multipart_field_plans(validation_schema)?;
                }
                Some(RequestBodyContent::SchemaLess { media_type }) => {
                    return Err(ServerCodegenError::UnsupportedRequestBody {
                        operation_id: operation.operation_id.clone(),
                        media_type: media_type.clone(),
                        reason: "request content has no schema to validate".to_string(),
                    });
                }
                Some(RequestBodyContent::Unsupported { media_types }) => {
                    return Err(ServerCodegenError::UnsupportedRequestBody {
                        operation_id: operation.operation_id.clone(),
                        media_type: media_types.join(", "),
                        reason: "the selected request body uses unsupported media content"
                            .to_string(),
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn validate_supported_server_outputs(
        &self,
        operations: &[&OperationInfo],
    ) -> Result<(), ServerCodegenError> {
        for operation in operations {
            if let Some(responses) = self
                .analysis
                .operation_responses
                .get(&operation.operation_id)
            {
                for (status, response) in responses {
                    // A Response Object may advertise multiple representations.
                    // Generation is viable whenever at least one buffered or SSE
                    // representation can be emitted; unsupported alternatives
                    // do not invalidate that supported path.
                    if response.has_content
                        && response.body.is_none()
                        && response.schema_name.is_none()
                        && !response.supports_streaming
                    {
                        return Err(ServerCodegenError::UnsupportedResponseContent {
                            operation_id: operation.operation_id.clone(),
                            status: status.clone(),
                            media_types: if response.unsupported_media_types.is_empty() {
                                "(schema-less content)".to_string()
                            } else {
                                response.unsupported_media_types.join(", ")
                            },
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn parameter_ident(&self, parameter: &ParameterInfo) -> syn::Ident {
        let generator = CodeGenerator::new(self.config.clone());
        CodeGenerator::to_field_ident(&generator.param_ident_str(parameter))
    }

    /// Allocate public response-enum identifiers without colliding with the
    /// retained models that the generated server modules glob-import.
    fn allocate_response_enum_names(&self, ops: &[&OperationInfo]) -> BTreeMap<String, syn::Ident> {
        let generator = CodeGenerator::new(self.config.clone());
        let mut used_names: std::collections::BTreeSet<String> = self
            .analysis
            .schemas
            .keys()
            .map(|name| generator.to_rust_type_name(name))
            .collect();

        // Parameter enums are emitted directly into api.rs rather than into
        // analysis.schemas, but share that module's type namespace with the
        // imported response enums.
        used_names.extend(
            ops.iter()
                .flat_map(|operation| operation.parameters.iter())
                .filter(|parameter| parameter.enum_values.is_some())
                .map(|parameter| parameter.rust_type.clone()),
        );

        // Selector ordering is user-controlled. Allocate in operation-id order
        // so reversing selectors cannot change generated public identifiers.
        let mut sorted_ops = ops.to_vec();
        sorted_ops.sort_by(|left, right| left.operation_id.cmp(&right.operation_id));

        let mut names = BTreeMap::new();
        for operation in sorted_ops {
            let operation_name = operation.operation_id.to_pascal_case();
            let preferred = format!("{operation_name}Response");
            let chosen = if used_names.insert(preferred.clone()) {
                preferred
            } else {
                let fallback = format!("{operation_name}ServerResponse");
                let mut candidate = fallback.clone();
                let mut suffix = 2;
                while !used_names.insert(candidate.clone()) {
                    candidate = format!("{fallback}{suffix}");
                    suffix += 1;
                }
                candidate
            };
            names.insert(operation.operation_id.clone(), format_ident!("{chosen}"));
        }
        names
    }

    fn validation_target(
        &self,
        bundle: Option<&super::validation::ValidationBundle>,
        operation: &OperationInfo,
        location: &str,
        parameter_name: Option<&str>,
    ) -> Result<Option<TokenStream>, ServerCodegenError> {
        let Some(bundle) = bundle else {
            return Ok(None);
        };
        let target = bundle
            .target_for(&operation.operation_id, location, parameter_name)
            .ok_or_else(|| {
                ServerCodegenError::Validation(format!(
                    "missing generated validator target for operation `{}` {location} `{}`",
                    operation.operation_id,
                    parameter_name.unwrap_or("body")
                ))
            })?;
        let ident = format_ident!("{}", target.constant);
        Ok(Some(quote! { super::validation::#ident }))
    }

    fn resolve_query_schema(&self, schema_name: &str) -> Option<&crate::analysis::AnalyzedSchema> {
        let mut current = schema_name;
        let mut visited = std::collections::HashSet::new();
        loop {
            if !visited.insert(current) {
                return None;
            }
            let schema = self.analysis.schemas.get(current)?;
            if let SchemaType::Reference { target } = &schema.schema_type {
                current = target;
            } else {
                return Some(schema);
            }
        }
    }

    fn query_object_properties(
        &self,
        parameter: &ParameterInfo,
    ) -> Option<&BTreeMap<String, crate::analysis::PropertyInfo>> {
        let schema = self.resolve_query_schema(parameter.schema_ref.as_deref()?)?;
        match &schema.schema_type {
            SchemaType::Object { properties, .. } => Some(properties),
            _ => None,
        }
    }

    fn query_object_required_properties(&self, parameter: &ParameterInfo) -> Vec<String> {
        let Some(schema) = parameter
            .schema_ref
            .as_deref()
            .and_then(|name| self.resolve_query_schema(name))
        else {
            return Vec::new();
        };
        let mut names = match &schema.schema_type {
            SchemaType::Object { required, .. } => required.iter().cloned().collect(),
            _ => Vec::new(),
        };
        names.sort();
        names
    }

    fn form_required_field_names(
        &self,
        operation: &OperationInfo,
    ) -> Result<Vec<String>, ServerCodegenError> {
        let Some(RequestBodyContent::FormUrlEncoded { schema_name, .. }) = &operation.request_body
        else {
            return Ok(Vec::new());
        };
        let schema = self.resolve_query_schema(schema_name).ok_or_else(|| {
            ServerCodegenError::UnsupportedRequestBody {
                operation_id: operation.operation_id.clone(),
                media_type: "application/x-www-form-urlencoded".to_string(),
                reason: format!("schema `{schema_name}` cannot be resolved"),
            }
        })?;
        let mut names = match &schema.schema_type {
            SchemaType::Object { required, .. } => required.iter().cloned().collect(),
            _ => Vec::new(),
        };
        names.sort();
        Ok(names)
    }

    fn query_property_is_scalar(
        &self,
        schema_type: &SchemaType,
        visited: &mut std::collections::HashSet<String>,
    ) -> bool {
        match schema_type {
            SchemaType::Primitive { .. }
            | SchemaType::StringEnum { .. }
            | SchemaType::ExtensibleEnum { .. } => true,
            SchemaType::Reference { target } if visited.insert(target.clone()) => {
                self.analysis.schemas.get(target).is_some_and(|schema| {
                    self.query_property_is_scalar(&schema.schema_type, visited)
                })
            }
            _ => false,
        }
    }

    fn validate_query_object(
        &self,
        operation: &OperationInfo,
        parameter: &ParameterInfo,
    ) -> Result<Vec<String>, ServerCodegenError> {
        let error = |reason: String| ServerCodegenError::UnsupportedQueryParameter {
            operation_id: operation.operation_id.clone(),
            parameter: parameter.name.clone(),
            reason,
        };
        let schema_name = parameter.schema_ref.as_deref().ok_or_else(|| {
            error("styled object parameter has no analyzed schema type".to_string())
        })?;
        let schema = self.resolve_query_schema(schema_name).ok_or_else(|| {
            error(format!(
                "query object schema `{schema_name}` could not be resolved"
            ))
        })?;
        let (properties, additional_properties) = match &schema.schema_type {
            SchemaType::Object {
                properties,
                additional_properties,
                ..
            } => (properties, additional_properties),
            _ => {
                return Err(error(format!(
                    "query schema `{schema_name}` does not resolve to a flat object"
                )));
            }
        };
        if !matches!(additional_properties, ObjectAdditionalProperties::Forbidden) {
            return Err(error(
                "styled object parameters with additionalProperties have an ambiguous wire namespace"
                    .to_string(),
            ));
        }
        for (property_name, property) in properties {
            if !self.query_property_is_scalar(
                &property.schema_type,
                &mut std::collections::HashSet::new(),
            ) {
                return Err(error(format!(
                    "property `{property_name}` is not scalar; nested arrays/objects are undefined for the generated query wire format"
                )));
            }
        }
        Ok(properties.keys().cloned().collect())
    }

    fn validate_query_parameters(
        &self,
        operations: &[&OperationInfo],
    ) -> Result<(), ServerCodegenError> {
        for operation in operations {
            let mut claimed_keys: BTreeMap<String, String> = BTreeMap::new();
            for parameter in operation
                .parameters
                .iter()
                .filter(|parameter| parameter.location == "query")
            {
                let mut keys = match &parameter.query_serialization {
                    Some(QuerySerialization::Unsupported { reason }) => {
                        return Err(ServerCodegenError::UnsupportedQueryParameter {
                            operation_id: operation.operation_id.clone(),
                            parameter: parameter.name.clone(),
                            reason: reason.clone(),
                        });
                    }
                    Some(
                        QuerySerialization::FormExplodedObject
                        | QuerySerialization::FormObject
                        | QuerySerialization::DeepObject,
                    ) => {
                        let property_keys = self.validate_query_object(operation, parameter)?;
                        if matches!(
                            parameter.query_serialization,
                            Some(QuerySerialization::FormExplodedObject)
                        ) {
                            property_keys
                        } else if matches!(
                            parameter.query_serialization,
                            Some(QuerySerialization::DeepObject)
                        ) {
                            property_keys
                                .into_iter()
                                .map(|property| format!("{}[{property}]", parameter.name))
                                .collect()
                        } else {
                            vec![parameter.name.clone()]
                        }
                    }
                    Some(QuerySerialization::FormExplodedNestedObject { .. }) => {
                        vec![parameter.name.clone()]
                    }
                    Some(
                        QuerySerialization::FormExplodedArray { .. }
                        | QuerySerialization::FormArray { .. }
                        | QuerySerialization::SimpleHeaderArray { .. },
                    )
                    | None => vec![parameter.name.clone()],
                };
                if matches!(
                    &parameter.query_serialization,
                    Some(
                        QuerySerialization::FormExplodedObject
                            | QuerySerialization::FormExplodedNestedObject { .. }
                            | QuerySerialization::FormObject
                            | QuerySerialization::DeepObject
                            | QuerySerialization::FormExplodedArray { .. }
                            | QuerySerialization::FormArray { .. }
                    )
                ) {
                    keys.push(format!("{}[]", parameter.name));
                }
                for key in keys {
                    if let Some(first_parameter) =
                        claimed_keys.insert(key.clone(), parameter.name.clone())
                    {
                        return Err(ServerCodegenError::AmbiguousQueryParameter {
                            operation_id: operation.operation_id.clone(),
                            wire_key: key,
                            first_parameter,
                            second_parameter: parameter.name.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn emit_mod(&self, validation_enabled: bool) -> TokenStream {
        let provenance_attribute = self.provenance_attribute();
        let validation_module = validation_enabled.then(|| quote! { pub(crate) mod validation; });
        quote! {
            //! Server scaffolding emitted by openapi-to-rust.
            //!
            //! Implement the per-tag trait(s) in `api` on your own struct,
            //! then build an `axum::Router` via `router::router(impl)`.

            #provenance_attribute

            pub mod api;
            pub mod errors;
            pub mod router;
            #validation_module

            pub use api::*;
            pub use errors::*;
            pub use router::*;
        }
    }

    fn emit_router(
        &self,
        groups: &BTreeMap<String, Vec<&OperationInfo>>,
        validation_bundle: Option<&super::validation::ValidationBundle>,
    ) -> Result<TokenStream, ServerCodegenError> {
        let provenance_attribute = self.provenance_attribute();
        let factories: Vec<TokenStream> = groups
            .iter()
            .map(|(tag, ops)| self.emit_router_for_trait(tag, ops, validation_bundle))
            .collect::<Result<_, _>>()?;

        // Per-op Query structs — one per op that has any query params.
        let query_structs: Vec<TokenStream> = groups
            .values()
            .flatten()
            .filter_map(|op| self.emit_query_struct(op))
            .collect();
        let has_query_parameters = groups.values().flatten().any(|operation| {
            operation
                .parameters
                .iter()
                .any(|parameter| parameter.location == "query")
        });
        let query_helpers = has_query_parameters.then(|| {
            quote! {
                fn __query_pairs(raw: ::std::option::Option<&str>) -> ::std::vec::Vec<(String, String)> {
                    raw.map(|query| {
                        ::url::form_urlencoded::parse(query.as_bytes())
                            .into_owned()
                            .collect()
                    })
                    .unwrap_or_default()
                }

                fn __validate_urlencoded(raw: &str) -> ::std::result::Result<(), String> {
                    let bytes = raw.as_bytes();
                    let mut index = 0;
                    while index < bytes.len() {
                        if bytes[index] == b'%' {
                            if index + 2 >= bytes.len()
                                || !bytes[index + 1].is_ascii_hexdigit()
                                || !bytes[index + 2].is_ascii_hexdigit()
                            {
                                return Err("malformed percent encoding".to_string());
                            }
                            index += 3;
                        } else {
                            index += 1;
                        }
                    }
                    Ok(())
                }

                fn __query_one(
                    pairs: &[(String, String)],
                    key: &str,
                ) -> ::std::result::Result<::std::option::Option<String>, String> {
                    let mut values = pairs
                        .iter()
                        .filter(|(candidate, _)| candidate == key)
                        .map(|(_, value)| value.clone());
                    let value = values.next();
                    if values.next().is_some() {
                        return Err(format!("query parameter `{key}` appeared more than once"));
                    }
                    Ok(value)
                }

                fn __decode_query_scalar<T>(
                    value: &str,
                    label: &str,
                ) -> ::std::result::Result<T, String>
                where
                    T: ::serde::de::DeserializeOwned,
                {
                    ::serde_json::from_value(::serde_json::Value::String(value.to_string()))
                        .or_else(|_| ::serde_json::from_str(value))
                        .map_err(|error| format!("invalid query value for `{label}`: {error}"))
                }

                fn __decode_query_object<T>(
                    fields: &[(String, String)],
                    label: &str,
                ) -> ::std::result::Result<T, String>
                where
                    T: ::serde::de::DeserializeOwned,
                {
                    let mut serializer =
                        ::url::form_urlencoded::Serializer::new(String::new());
                    for (key, value) in fields {
                        serializer.append_pair(key, value);
                    }
                    ::serde_urlencoded::from_str(&serializer.finish())
                        .map_err(|error| format!("invalid query object `{label}`: {error}"))
                }

                fn __query_empty_marker(
                    pairs: &[(String, String)],
                    key: &str,
                ) -> ::std::result::Result<bool, String> {
                    let marker = format!("{key}[]");
                    match __query_one(pairs, &marker)? {
                        Some(value) if value.is_empty() => Ok(true),
                        Some(_) => Err(format!(
                            "zero-cardinality marker `{marker}` must have an empty value"
                        )),
                        None => Ok(false),
                    }
                }
            }
        });

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

        Ok(quote! {
            //! Router factories — one per trait. Each takes any
            //! `T: <TraitName> + Clone + Send + Sync + 'static` and
            //! returns an `axum::Router` with state pre-attached.

            #provenance_attribute

            use super::api::*;
            use super::errors::*;
            // Pull schemas directly from the types module (always a
            // sibling of mod.rs). Doesn't rely on the parent module
            // re-exporting types::*, so users can mount the generated
            // tree at any path without rewriting these imports.
            #[allow(unused_imports)]
            use super::super::types::*;

            #query_helpers

            #(#query_structs)*

            #(#factories)*

            #combined
        })
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

    fn emit_router_for_trait(
        &self,
        tag: &str,
        ops: &[&OperationInfo],
        validation_bundle: Option<&super::validation::ValidationBundle>,
    ) -> Result<TokenStream, ServerCodegenError> {
        let trait_ident = trait_ident_for_tag(tag);
        let fn_ident = format_ident!("{}_router", trait_ident.to_string().to_snake_case());

        let mut routes: Vec<TokenStream> = Vec::new();
        let mut custom_by_path: BTreeMap<String, Vec<(String, syn::Ident)>> = BTreeMap::new();
        for op in ops {
            let handler = format_ident!("{}_handler", op.operation_id.to_snake_case());
            let path = openapi_to_axum_path(&op.path)?;
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
            .map(|op| self.emit_axum_handler(&trait_ident, op, validation_bundle))
            .collect::<Result<_, _>>()?;

        let doc = format!(" Build an axum::Router for the `{trait_ident}` trait.");
        let max_body_bytes = self.server.validation.max_body_bytes;

        Ok(quote! {
            #[doc = #doc]
            pub fn #fn_ident<T>(api: T) -> ::axum::Router
            where
                T: #trait_ident + Clone + Send + Sync + 'static,
            {
                ::axum::Router::new()
                    #(#routes)*
                    .layer(::axum::extract::DefaultBodyLimit::max(#max_body_bytes))
                    .with_state(api)
            }

            #(#custom_dispatchers)*

            #(#handlers)*
        })
    }

    fn emit_axum_handler(
        &self,
        trait_ident: &syn::Ident,
        op: &OperationInfo,
        validation_bundle: Option<&super::validation::ValidationBundle>,
    ) -> Result<TokenStream, ServerCodegenError> {
        let handler_ident = format_ident!("{}_handler", op.operation_id.to_snake_case());
        let trait_method = format_ident!("{}", op.operation_id.to_snake_case());

        // Build extractor list + call argument list.
        let mut extractors: Vec<TokenStream> =
            vec![quote! { ::axum::extract::State(api): ::axum::extract::State<T> }];
        let mut call_args: Vec<TokenStream> = Vec::new();

        // Path parameters. With validation enabled, extract by wire name so
        // declaration order cannot drift from the route template and all
        // malformed values use the public rejection profile.
        let path_params: Vec<&_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "path")
            .collect();
        let path_has_affixes = path_params
            .iter()
            .any(|parameter| path_parameter_affixes(&op.path, &parameter.name).is_some());
        let mut path_decode = TokenStream::new();
        if !path_params.is_empty() && (validation_bundle.is_some() || path_has_affixes) {
            extractors.push(quote! {
                __path_result: ::std::result::Result<
                    ::axum::extract::Path<::std::collections::HashMap<String, String>>,
                    ::axum::extract::rejection::PathRejection,
                >
            });
            let mut decoders = Vec::new();
            for parameter in &path_params {
                let ident = self.parameter_ident(parameter);
                let ty = self.query_parameter_type(parameter);
                let wire = parameter.name.as_str();
                let location = parameter_location("path", wire);
                let target = self.validation_target(validation_bundle, op, "path", Some(wire))?;
                let string_wire = self.parameter_schema_is_string(parameter);
                let strip_affixes = match path_parameter_affixes(&op.path, wire) {
                    Some((prefix, suffix)) => quote! {
                        let raw = match raw
                            .strip_prefix(#prefix)
                            .and_then(|value| value.strip_suffix(#suffix))
                        {
                            Some(value) => value.to_string(),
                            None => return ::axum::response::IntoResponse::into_response(
                                ::axum::http::StatusCode::NOT_FOUND
                            ),
                        };
                    },
                    None => TokenStream::new(),
                };
                let decode = if let Some(target) = target {
                    quote! {
                        match super::validation::decode_parameter(
                            &raw, #target, #location, #string_wire,
                        ) {
                            Ok(value) => value,
                            Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                        }
                    }
                } else {
                    quote! {
                        match ::serde_json::from_value(::serde_json::Value::String(raw.clone()))
                            .or_else(|_| ::serde_json::from_str(&raw))
                        {
                            Ok(value) => value,
                            Err(_) => return ::axum::response::IntoResponse::into_response(
                                ::axum::http::StatusCode::BAD_REQUEST
                            ),
                        }
                    }
                };
                decoders.push(quote! {
                    let #ident: #ty = match __path_values.remove(#wire) {
                        Some(raw) => {
                            #strip_affixes
                            #decode
                        },
                        None => return ::axum::response::IntoResponse::into_response(
                            ::axum::http::StatusCode::INTERNAL_SERVER_ERROR
                        ),
                    };
                });
                call_args.push(quote! { #ident });
            }
            path_decode = quote! {
                let ::axum::extract::Path(mut __path_values) = match __path_result {
                    Ok(path) => path,
                    Err(_) => return ::axum::response::IntoResponse::into_response(
                        ::axum::http::StatusCode::BAD_REQUEST
                    ),
                };
                #(#decoders)*
            };
        } else if !path_params.is_empty() {
            let idents: Vec<syn::Ident> = path_params
                .iter()
                .map(|p| self.parameter_ident(p))
                .collect();
            let types: Vec<TokenStream> = path_params
                .iter()
                .map(|p| self.query_parameter_type(p))
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
        let mut query_validation_checks: Vec<TokenStream> = Vec::new();
        let mut raw_query_validation_checks: Vec<TokenStream> = Vec::new();
        let mut query_decode = TokenStream::new();
        if !query_params.is_empty() {
            let query_ident = format_ident!("{}Query", op.operation_id.to_pascal_case());
            let decode_ident = format_ident!("__decode_{}_query", op.operation_id.to_snake_case());
            extractors.push(quote! {
                ::axum::extract::RawQuery(__raw_query): ::axum::extract::RawQuery
            });
            query_decode = if validation_bundle.is_some() {
                quote! {
                    let __q: #query_ident = match #decode_ident(__raw_query.as_deref()) {
                        Ok(query) => query,
                        Err(_) => return ::axum::response::IntoResponse::into_response(
                            super::validation::malformed_parameter("/query")
                        ),
                    };
                }
            } else {
                quote! {
                    let __q: #query_ident = match #decode_ident(__raw_query.as_deref()) {
                        Ok(query) => query,
                        Err(message) => return ::axum::response::IntoResponse::into_response(
                            (
                                ::axum::http::StatusCode::BAD_REQUEST,
                                ::axum::Json(::serde_json::json!({ "error": message })),
                            )
                        ),
                    };
                }
            };
            for p in &query_params {
                let f = self.parameter_ident(p);
                let wire = p.name.as_str();
                let location = parameter_location("query", wire);
                let target = self.validation_target(validation_bundle, op, "query", Some(wire))?;
                if p.query_serialization.is_none() && self.parameter_schema_is_string(p) {
                    if let Some(target) = target.as_ref() {
                        raw_query_validation_checks.push(quote! {
                            if let Ok(Some(raw)) = __query_one(&__raw_query_pairs, #wire) {
                                if let Err(rejection) = super::validation::validate_string_parameter(
                                    #target, #location, &raw,
                                ) {
                                    return ::axum::response::IntoResponse::into_response(rejection);
                                }
                            }
                        });
                    }
                }
                if p.required {
                    required_query_checks.push(if validation_bundle.is_some() {
                        quote! {
                            let #f = match __q.#f {
                                Some(v) => v,
                                None => return ::axum::response::IntoResponse::into_response(
                                    super::validation::missing_parameter(#location)
                                ),
                            };
                        }
                    } else {
                        let missing_msg = format!("missing required query parameter `{wire}`");
                        quote! {
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
                        }
                    });
                    if let Some(target) = target {
                        query_validation_checks.push(quote! {
                            if let Err(rejection) = super::validation::validate_parameter(
                                #target, #location, &#f,
                            ) {
                                return ::axum::response::IntoResponse::into_response(rejection);
                            }
                        });
                    }
                    call_args.push(quote! { #f });
                } else {
                    if let Some(target) = target {
                        query_validation_checks.push(quote! {
                            if let Some(value) = &__q.#f {
                                if let Err(rejection) = super::validation::validate_parameter(
                                    #target, #location, value,
                                ) {
                                    return ::axum::response::IntoResponse::into_response(rejection);
                                }
                            }
                        });
                    }
                    call_args.push(quote! { __q.#f });
                }
            }
        }

        // Scalar header and cookie parameters are decoded to their generated
        // Rust types before schema validation. Raw transport/parser errors are
        // deliberately discarded at the public boundary.
        let header_params: Vec<&_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "header")
            .collect();
        let cookie_params: Vec<&_> = op
            .parameters
            .iter()
            .filter(|p| p.location == "cookie")
            .collect();
        let mut parameter_decode_checks: Vec<TokenStream> = Vec::new();
        if !header_params.is_empty() || !cookie_params.is_empty() {
            extractors.push(quote! { __headers: ::axum::http::HeaderMap });
        }
        if !header_params.is_empty() {
            for p in &header_params {
                let wire = p.name.as_str();
                let ident = self.parameter_ident(p);
                let ty = self.query_parameter_type(p);
                let location = parameter_location("header", wire);
                let target = self.validation_target(validation_bundle, op, "header", Some(wire))?;
                let string_wire = self.parameter_schema_is_string(p);
                if matches!(
                    p.query_serialization,
                    Some(QuerySerialization::SimpleHeaderArray { .. })
                ) {
                    let decode_array = quote! {
                        raw.split(',')
                            .map(|item| __decode_query_scalar(item, #wire))
                            .collect::<::std::result::Result<#ty, _>>()
                    };
                    if p.required {
                        parameter_decode_checks.push(quote! {
                            let mut __values = __headers.get_all(#wire).iter();
                            let #ident: #ty = match (__values.next(), __values.next()) {
                                (Some(value), None) => match value.to_str() {
                                    Ok(raw) => match #decode_array {
                                        Ok(value) => value,
                                        Err(_) => return ::axum::response::IntoResponse::into_response(
                                            super::validation::malformed_parameter(#location)
                                        ),
                                    },
                                    Err(_) => return ::axum::response::IntoResponse::into_response(
                                        super::validation::malformed_parameter(#location)
                                    ),
                                },
                                (None, _) => return ::axum::response::IntoResponse::into_response(
                                    super::validation::missing_parameter(#location)
                                ),
                                _ => return ::axum::response::IntoResponse::into_response(
                                    super::validation::malformed_parameter(#location)
                                ),
                            };
                        });
                        if let Some(target) = target {
                            parameter_decode_checks.push(quote! {
                                if let Err(rejection) = super::validation::validate_parameter(
                                    #target, #location, &#ident,
                                ) {
                                    return ::axum::response::IntoResponse::into_response(rejection);
                                }
                            });
                        }
                        call_args.push(quote! { #ident });
                    } else {
                        parameter_decode_checks.push(quote! {
                            let mut __values = __headers.get_all(#wire).iter();
                            let #ident: ::std::option::Option<#ty> = match (__values.next(), __values.next()) {
                                (Some(value), None) => match value.to_str() {
                                    Ok(raw) => match #decode_array {
                                        Ok(value) => Some(value),
                                        Err(_) => return ::axum::response::IntoResponse::into_response(
                                            super::validation::malformed_parameter(#location)
                                        ),
                                    },
                                    Err(_) => return ::axum::response::IntoResponse::into_response(
                                        super::validation::malformed_parameter(#location)
                                    ),
                                },
                                (None, _) => None,
                                _ => return ::axum::response::IntoResponse::into_response(
                                    super::validation::malformed_parameter(#location)
                                ),
                            };
                        });
                        if let Some(target) = target {
                            parameter_decode_checks.push(quote! {
                                if let Some(value) = &#ident {
                                    if let Err(rejection) = super::validation::validate_parameter(
                                        #target, #location, value,
                                    ) {
                                        return ::axum::response::IntoResponse::into_response(rejection);
                                    }
                                }
                            });
                        }
                        call_args.push(quote! { #ident });
                    }
                    continue;
                }
                if p.required {
                    if let Some(target) = target {
                        parameter_decode_checks.push(quote! {
                            let mut __values = __headers.get_all(#wire).iter();
                            let #ident: #ty = match (__values.next(), __values.next()) {
                                (Some(value), None) => match value.to_str() {
                                    Ok(raw) => match super::validation::decode_parameter(
                                        raw, #target, #location, #string_wire,
                                    ) {
                                        Ok(value) => value,
                                        Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                                    },
                                    Err(_) => return ::axum::response::IntoResponse::into_response(
                                        super::validation::malformed_parameter(#location)
                                    ),
                                },
                                (None, _) => return ::axum::response::IntoResponse::into_response(
                                    super::validation::missing_parameter(#location)
                                ),
                                _ => return ::axum::response::IntoResponse::into_response(
                                    super::validation::malformed_parameter(#location)
                                ),
                            };
                        });
                    } else {
                        parameter_decode_checks.push(quote! {
                            let #ident: #ty = match __headers.get(#wire).and_then(|v| v.to_str().ok()) {
                                Some(raw) => match __decode_query_scalar(raw, #wire) {
                                    Ok(value) => value,
                                    Err(_) => return ::axum::http::StatusCode::BAD_REQUEST.into_response(),
                                },
                                None => return ::axum::http::StatusCode::BAD_REQUEST.into_response(),
                            };
                        });
                    }
                    call_args.push(quote! { #ident });
                } else {
                    if let Some(target) = target {
                        parameter_decode_checks.push(quote! {
                            let mut __values = __headers.get_all(#wire).iter();
                            let #ident: ::std::option::Option<#ty> = match (__values.next(), __values.next()) {
                                (Some(value), None) => match value.to_str() {
                                    Ok(raw) => match super::validation::decode_parameter(raw, #target, #location, #string_wire) {
                                        Ok(value) => Some(value),
                                        Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                                    },
                                    Err(_) => return ::axum::response::IntoResponse::into_response(
                                        super::validation::malformed_parameter(#location)
                                    ),
                                },
                                (None, _) => None,
                                _ => return ::axum::response::IntoResponse::into_response(
                                    super::validation::malformed_parameter(#location)
                                ),
                            };
                        });
                    } else {
                        parameter_decode_checks.push(quote! {
                            let #ident: ::std::option::Option<#ty> = __headers.get(#wire)
                                .and_then(|value| value.to_str().ok())
                                .and_then(|raw| __decode_query_scalar(raw, #wire).ok());
                        });
                    }
                    call_args.push(quote! { #ident });
                }
            }
        }
        if !cookie_params.is_empty() {
            parameter_decode_checks.push(quote! {
                let mut __cookies = match super::validation::parse_cookies(&__headers) {
                    Ok(cookies) => cookies,
                    Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                };
            });
            for p in &cookie_params {
                let wire = p.name.as_str();
                let ident = self.parameter_ident(p);
                let ty = self.query_parameter_type(p);
                let location = parameter_location("cookie", wire);
                let target = self
                    .validation_target(validation_bundle, op, "cookie", Some(wire))?
                    .ok_or_else(|| {
                        ServerCodegenError::Validation(format!(
                            "cookie extraction requires validation for operation `{}`",
                            op.operation_id
                        ))
                    })?;
                let string_wire = self.parameter_schema_is_string(p);
                if p.required {
                    parameter_decode_checks.push(quote! {
                        let #ident: #ty = match __cookies.remove(#wire) {
                            Some(raw) => match super::validation::decode_parameter(&raw, #target, #location, #string_wire) {
                                Ok(value) => value,
                                Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                            },
                            None => return ::axum::response::IntoResponse::into_response(
                                super::validation::missing_parameter(#location)
                            ),
                        };
                    });
                    call_args.push(quote! { #ident });
                } else {
                    parameter_decode_checks.push(quote! {
                        let #ident: ::std::option::Option<#ty> = match __cookies.remove(#wire) {
                            Some(raw) => match super::validation::decode_parameter(&raw, #target, #location, #string_wire) {
                                Ok(value) => Some(value),
                                Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                            },
                            None => None,
                        };
                    });
                    call_args.push(quote! { #ident });
                }
            }
        }

        // Body
        let mut body_decode = TokenStream::new();
        let body_ty_opt = body_type(op);
        if let Some(body_ty) = &body_ty_opt {
            let body_ty_tokens = self.model_type(body_ty);
            let transport_body = match &op.request_body {
                Some(RequestBodyContent::OctetStream { media_type }) => {
                    Some((format_ident!("decode_binary_body"), media_type.clone()))
                }
                Some(RequestBodyContent::Binary { media_type }) => {
                    Some((format_ident!("decode_binary_body"), media_type.clone()))
                }
                Some(RequestBodyContent::TextPlain { media_type }) => {
                    Some((format_ident!("decode_text_body"), media_type.clone()))
                }
                _ => None,
            };
            let validated_json = matches!(&op.request_body, Some(RequestBodyContent::Json { .. }))
                && validation_bundle.is_some();
            let validated_form = matches!(
                &op.request_body,
                Some(RequestBodyContent::FormUrlEncoded { .. })
            ) && validation_bundle.is_some();
            let typed_multipart =
                matches!(&op.request_body, Some(RequestBodyContent::Multipart { .. }));
            if let Some((decoder, media_type)) = transport_body {
                extractors.push(quote! { __request: ::axum::extract::Request });
                let required = op.request_body_required;
                let max_body_bytes = self.server.validation.max_body_bytes;
                body_decode = quote! {
                    let body: ::std::option::Option<#body_ty_tokens> =
                        match super::validation::#decoder(
                            __request,
                            #media_type,
                            #required,
                            #max_body_bytes,
                        ).await {
                            Ok(body) => body,
                            Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                        };
                };
                if required {
                    body_decode.extend(quote! {
                        let body = match body {
                            Some(body) => body,
                            None => return ::axum::response::IntoResponse::into_response(
                                super::validation::generated_contract_error()
                            ),
                        };
                    });
                }
                call_args.push(quote! { body });
            } else if typed_multipart {
                extractors.push(quote! { __request: ::axum::extract::Request });
                let Some(RequestBodyContent::Multipart {
                    validation_schema, ..
                }) = &op.request_body
                else {
                    return Err(ServerCodegenError::Internal(
                        "typed multipart body lost its schema".to_string(),
                    ));
                };
                let plans = self.multipart_field_plans(validation_schema)?;
                let multipart_validation = self
                    .validation_target(validation_bundle, op, "body", None)?
                    .map(|target| {
                        quote! {
                            if let Err(rejection) = super::validation::validate_parameter(
                                #target,
                                "/body",
                                &::serde_json::Value::Object(validation_object),
                            ) {
                                return ::axum::response::IntoResponse::into_response(rejection);
                            }
                        }
                    })
                    .unwrap_or_default();
                let mut binary_locals = Vec::new();
                let mut binary_validation_arms = Vec::new();
                let mut binary_patches = Vec::new();
                for plan in plans
                    .iter()
                    .filter(|plan| matches!(plan.kind, MultipartFieldKind::Binary))
                {
                    let wire_name = &plan.wire_name;
                    let field_ident = &plan.field_ident;
                    let slot = format_ident!("__multipart_binary_{}", field_ident);
                    binary_locals.push(quote! {
                        let mut #slot: ::std::option::Option<::bytes::Bytes> = None;
                    });
                    binary_validation_arms.push(quote! {
                        #wire_name => ::serde_json::Value::String(
                            "x".repeat(#slot.as_ref().map_or(0, ::bytes::Bytes::len))
                        ),
                    });
                    binary_patches.push(match (self.config.types.binary, plan.required) {
                        (crate::type_mapping::BinaryStrategy::Bytes, true) => quote! {
                            body.#field_ident = match #slot {
                                Some(bytes) => bytes,
                                None => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::UNPROCESSABLE_ENTITY, "missing required multipart field")
                                ),
                            };
                        },
                        (crate::type_mapping::BinaryStrategy::Bytes, false) => quote! {
                            body.#field_ident = #slot;
                        },
                        (crate::type_mapping::BinaryStrategy::VecU8, true) => quote! {
                            body.#field_ident = match #slot {
                                Some(bytes) => bytes.to_vec(),
                                None => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::UNPROCESSABLE_ENTITY, "missing required multipart field")
                                ),
                            };
                        },
                        (crate::type_mapping::BinaryStrategy::VecU8, false) => quote! {
                            body.#field_ident = #slot.map(|bytes| bytes.to_vec());
                        },
                        (crate::type_mapping::BinaryStrategy::String, _) => unreachable!(
                            "string-backed binary fields must use multipart text extraction"
                        ),
                    });
                }
                let mut field_arms = Vec::new();
                for plan in plans {
                    let wire_name = plan.wire_name;
                    let binary_slot = format_ident!("__multipart_binary_{}", plan.field_ident);
                    let decode = match plan.kind {
                        MultipartFieldKind::Binary => quote! {
                            let bytes = match field.bytes().await {
                                Ok(bytes) => bytes,
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart field")
                                ),
                            };
                            #binary_slot = Some(bytes);
                            ::serde_json::Value::Array(Vec::new())
                        },
                        MultipartFieldKind::String => quote! {
                            match field.text().await {
                                Ok(text) => ::serde_json::Value::String(text),
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart text field")
                                ),
                            }
                        },
                        MultipartFieldKind::Integer => quote! {
                            let text = match field.text().await {
                                Ok(text) => text,
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart integer field")
                                ),
                            };
                            match text.parse::<i64>() {
                                Ok(value) => ::serde_json::Value::Number(value.into()),
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart integer field")
                                ),
                            }
                        },
                        MultipartFieldKind::UnsignedInteger => quote! {
                            let text = match field.text().await {
                                Ok(text) => text,
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart unsigned integer field")
                                ),
                            };
                            match text.parse::<u64>() {
                                Ok(value) => ::serde_json::Value::Number(value.into()),
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart unsigned integer field")
                                ),
                            }
                        },
                        MultipartFieldKind::Number => quote! {
                            let text = match field.text().await {
                                Ok(text) => text,
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart number field")
                                ),
                            };
                            match text.parse::<f64>().ok().and_then(::serde_json::Number::from_f64) {
                                Some(value) => ::serde_json::Value::Number(value),
                                None => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart number field")
                                ),
                            }
                        },
                        MultipartFieldKind::Boolean => quote! {
                            let text = match field.text().await {
                                Ok(text) => text,
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart boolean field")
                                ),
                            };
                            match text.parse::<bool>() {
                                Ok(value) => ::serde_json::Value::Bool(value),
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::BAD_REQUEST, "invalid multipart boolean field")
                                ),
                            }
                        },
                    };
                    field_arms.push(quote! {
                        #wire_name => { #decode }
                    });
                }
                let required = op.request_body_required;
                body_decode = quote! {
                    let __has_multipart_content_type = __request.headers()
                        .get(::axum::http::header::CONTENT_TYPE)
                        .is_some();
                    let body: ::std::option::Option<#body_ty_tokens> =
                        if !__has_multipart_content_type && !#required {
                            None
                        } else {
                            let mut multipart = match <::axum::extract::Multipart as ::axum::extract::FromRequest<T>>::from_request(__request, &api).await {
                                Ok(multipart) => multipart,
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE, "invalid multipart request")
                                ),
                            };
                            let mut object = ::serde_json::Map::new();
                            let mut validation_object = ::serde_json::Map::new();
                            #(#binary_locals)*
                            loop {
                                let field = match multipart.next_field().await {
                                    Ok(Some(field)) => field,
                                    Ok(None) => break,
                                    Err(_) => return ::axum::response::IntoResponse::into_response(
                                        (::axum::http::StatusCode::BAD_REQUEST, "malformed multipart request")
                                    ),
                                };
                                let name = match field.name() {
                                    Some(name) => name.to_string(),
                                    None => return ::axum::response::IntoResponse::into_response(
                                        (::axum::http::StatusCode::BAD_REQUEST, "multipart field has no name")
                                    ),
                                };
                                if object.contains_key(&name) {
                                    return ::axum::response::IntoResponse::into_response(
                                        (::axum::http::StatusCode::BAD_REQUEST, "duplicate multipart field")
                                    );
                                }
                                let value = match name.as_str() {
                                    #(#field_arms,)*
                                    _ => return ::axum::response::IntoResponse::into_response(
                                        (::axum::http::StatusCode::UNPROCESSABLE_ENTITY, "unknown multipart field")
                                    ),
                                };
                                let validation_value = match name.as_str() {
                                    #(#binary_validation_arms)*
                                    _ => value.clone(),
                                };
                                object.insert(name.clone(), value);
                                validation_object.insert(name, validation_value);
                            }
                            #multipart_validation
                            match ::serde_json::from_value::<#body_ty_tokens>(::serde_json::Value::Object(object)) {
                                Ok(mut body) => {
                                    #(#binary_patches)*
                                    Some(body)
                                },
                                Err(_) => return ::axum::response::IntoResponse::into_response(
                                    (::axum::http::StatusCode::UNPROCESSABLE_ENTITY, "invalid multipart body")
                                ),
                            }
                        };
                };
                if required {
                    body_decode.extend(quote! {
                        let body = match body {
                            Some(body) => body,
                            None => return ::axum::response::IntoResponse::into_response(
                                (::axum::http::StatusCode::UNPROCESSABLE_ENTITY, "missing required multipart body")
                            ),
                        };
                    });
                }
                call_args.push(quote! { body });
            } else if validated_json {
                extractors.push(quote! { __request: ::axum::extract::Request });
                let Some(RequestBodyContent::Json { media_type, .. }) = &op.request_body else {
                    return Err(ServerCodegenError::Internal(
                        "validated JSON body lost its media type".to_string(),
                    ));
                };
                let target = self
                    .validation_target(validation_bundle, op, "body", None)?
                    .ok_or_else(|| {
                        ServerCodegenError::Validation(format!(
                            "validation target unexpectedly disabled for operation `{}` body",
                            op.operation_id
                        ))
                    })?;
                let required = op.request_body_required;
                let max_body_bytes = self.server.validation.max_body_bytes;
                body_decode = if required {
                    quote! {
                        let body: #body_ty_tokens = match super::validation::decode_json_body::<#body_ty_tokens>(
                            __request,
                            #target,
                            #media_type,
                            true,
                            #max_body_bytes,
                        ).await {
                            Ok(Some(body)) => body,
                            Ok(None) => return ::axum::response::IntoResponse::into_response(
                                super::validation::generated_contract_error()
                            ),
                            Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                        };
                    }
                } else {
                    quote! {
                        let body: ::std::option::Option<#body_ty_tokens> =
                            match super::validation::decode_json_body::<#body_ty_tokens>(
                                __request,
                                #target,
                                #media_type,
                                false,
                                #max_body_bytes,
                            ).await {
                                Ok(body) => body,
                                Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                            };
                    }
                };
                call_args.push(quote! { body });
            } else if validated_form {
                extractors.push(quote! { __request: ::axum::extract::Request });
                let Some(RequestBodyContent::FormUrlEncoded { media_type, .. }) = &op.request_body
                else {
                    return Err(ServerCodegenError::Internal(
                        "validated form body lost its media type".to_string(),
                    ));
                };
                let target = self
                    .validation_target(validation_bundle, op, "body", None)?
                    .ok_or_else(|| {
                        ServerCodegenError::Validation(format!(
                            "validation target unexpectedly disabled for operation `{}` body",
                            op.operation_id
                        ))
                    })?;
                let allowed_fields = self.form_field_names(op)?;
                let required_fields = self.form_required_field_names(op)?;
                let required = op.request_body_required;
                let max_body_bytes = self.server.validation.max_body_bytes;
                body_decode = quote! {
                    let body: ::std::option::Option<#body_ty_tokens> =
                        match super::validation::decode_form_body::<#body_ty_tokens>(
                            __request,
                            #target,
                            #media_type,
                            #required,
                            #max_body_bytes,
                            &[#(#allowed_fields),*],
                            &[#(#required_fields),*],
                        ).await {
                            Ok(body) => body,
                            Err(rejection) => return ::axum::response::IntoResponse::into_response(rejection),
                        };
                };
                if required {
                    body_decode.extend(quote! {
                        let body = match body {
                            Some(body) => body,
                            None => return ::axum::response::IntoResponse::into_response(
                                super::validation::generated_contract_error()
                            ),
                        };
                    });
                }
                call_args.push(quote! { body });
            } else if op.request_body_required {
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

        // Keep referencing trait_ident so the where-bound name is
        // visible to downstream readers — clippy would otherwise flag
        // it as unused in some configurations.
        let _ = trait_ident;
        let raw_query_validation = (!raw_query_validation_checks.is_empty()).then(|| {
            quote! {
                if let Some(raw) = __raw_query.as_deref() {
                    if __validate_urlencoded(raw).is_err() {
                        return ::axum::response::IntoResponse::into_response(
                            super::validation::malformed_parameter("/query")
                        );
                    }
                }
                let __raw_query_pairs = __query_pairs(__raw_query.as_deref());
                #(#raw_query_validation_checks)*
            }
        });

        // Handler returns `axum::response::Response` so the required-
        // param short-circuit (400 BadRequest) and the trait method's
        // typed response enum (via IntoResponse) can both flow out
        // through the same return type.
        Ok(quote! {
            async fn #handler_ident<T>(
                #(#extractors),*
            ) -> ::axum::response::Response
            where
                T: super::api::#trait_ident + Clone + Send + Sync + 'static,
            {
                #path_decode
                #raw_query_validation
                #query_decode
                #(#required_query_checks)*
                #(#query_validation_checks)*
                #(#parameter_decode_checks)*
                #body_decode
                ::axum::response::IntoResponse::into_response(
                    api.#trait_method(#(#call_args),*).await,
                )
            }
        })
    }

    fn emit_api(
        &self,
        groups: &BTreeMap<String, Vec<&OperationInfo>>,
        response_enum_names: &BTreeMap<String, syn::Ident>,
    ) -> TokenStream {
        let provenance_attribute = self.provenance_attribute();
        let traits: Vec<TokenStream> = groups
            .iter()
            .map(|(tag, ops)| self.emit_trait(tag, ops, response_enum_names))
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

            #provenance_attribute

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

    fn emit_trait(
        &self,
        tag: &str,
        ops: &[&OperationInfo],
        response_enum_names: &BTreeMap<String, syn::Ident>,
    ) -> TokenStream {
        let trait_ident = trait_ident_for_tag(tag);
        let methods: Vec<TokenStream> = ops
            .iter()
            .map(|op| self.emit_method_sig(op, response_enum_names))
            .collect();
        let doc = format!(" Operations under the `{tag}` tag.");
        quote! {
            #[doc = #doc]
            #[async_trait::async_trait]
            pub trait #trait_ident: Send + Sync + 'static {
                #(#methods)*
            }
        }
    }

    fn emit_method_sig(
        &self,
        op: &OperationInfo,
        response_enum_names: &BTreeMap<String, syn::Ident>,
    ) -> TokenStream {
        let name = format_ident!("{}", op.operation_id.to_snake_case());
        let response_ty = &response_enum_names[&op.operation_id];

        // Order: path → query → header → body. Required params keep
        // their declared rust_type; optional params wrap in Option<…>.
        // This mirrors what the router handler extracts so positional
        // ordering matches the call site exactly.
        let mut params: Vec<TokenStream> = Vec::new();
        for p in &op.parameters {
            if p.location == "path" {
                let ident = self.parameter_ident(p);
                let ty = self.query_parameter_type(p);
                params.push(quote! { #ident: #ty });
            }
        }
        for p in &op.parameters {
            if p.location == "query" {
                let ident = self.parameter_ident(p);
                let ty = self.query_parameter_type(p);
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
                let ident = self.parameter_ident(p);
                let ty = self.query_parameter_type(p);
                if p.required {
                    params.push(quote! { #ident: #ty });
                } else {
                    params.push(quote! { #ident: ::std::option::Option<#ty> });
                }
            }
        }
        for p in &op.parameters {
            if p.location == "cookie" {
                let ident = self.parameter_ident(p);
                let ty = self.query_parameter_type(p);
                if p.required {
                    params.push(quote! { #ident: #ty });
                } else {
                    params.push(quote! { #ident: ::std::option::Option<#ty> });
                }
            }
        }
        if let Some(body) = body_type(op) {
            let body_ty = self.model_type(&body);
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
    /// has any query parameters. An operation-specific decoder fills it from
    /// Axum's raw query so repeated and structured keys remain observable.
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
        let decode_ident = format_ident!("__decode_{}_query", op.operation_id.to_snake_case());
        let mut fields = Vec::new();
        let mut decoders = Vec::new();
        let mut field_idents = Vec::new();
        for parameter in query_params {
            let field_ident = self.parameter_ident(parameter);
            let field_type = self.query_parameter_type(parameter);
            let wire_name = parameter.name.as_str();
            fields.push(quote! {
                pub #field_ident: ::std::option::Option<#field_type>
            });
            field_idents.push(field_ident.clone());

            let decoder = match &parameter.query_serialization {
                Some(QuerySerialization::FormExplodedArray { item_type }) => {
                    if let crate::analysis::ArrayItemType::FlatStructRef { properties, .. } =
                        item_type
                    {
                        let property_names = properties
                            .iter()
                            .map(|property| property.wire_name.clone())
                            .collect::<Vec<_>>();
                        // AWS query-protocol flat structures arrive as
                        // `param.N.Prop=value`; group by N and decode each
                        // group as one JSON object so serde fills the struct.
                        quote! {
                            let #field_ident = {
                                let empty_marker = __query_empty_marker(&__pairs, #wire_name)?;
                                let prefix = concat!(#wire_name, ".");
                                let mut groups: ::std::collections::BTreeMap<usize, ::serde_json::Map<String, ::serde_json::Value>> = ::std::collections::BTreeMap::new();
                                let allowed = [#(#property_names),*];
                                for (key, value) in &__pairs {
                                    let Some(rest) = key.strip_prefix(prefix) else { continue };
                                    if let Some(index) = rest.strip_suffix("[]").and_then(|index| index.parse::<usize>().ok()) {
                                        groups.entry(index).or_default();
                                        continue;
                                    }
                                    let Some((index, property)) = rest.split_once('.') else { continue };
                                    let Ok(index) = index.parse::<usize>() else { continue };
                                    if !allowed.contains(&property) {
                                        continue;
                                    }
                                    groups
                                        .entry(index)
                                        .or_default()
                                        .insert(property.to_string(), ::serde_json::Value::String(value.clone()));
                                }
                                if empty_marker && !groups.is_empty() {
                                    return Err(format!(
                                        "query array `{}` cannot combine values with its empty marker",
                                        #wire_name,
                                    ));
                                }
                                if empty_marker {
                                    Some(Vec::new())
                                } else if groups.is_empty() {
                                    None
                                } else {
                                    let mut values = Vec::with_capacity(groups.len());
                                    for (_, object) in groups {
                                        values.push(
                                            ::serde_json::from_value(::serde_json::Value::Object(object))
                                                .map_err(|error| format!("invalid query structure for `{}`: {error}", #wire_name))?,
                                        );
                                    }
                                    Some(values)
                                }
                            };
                        }
                    } else if let crate::analysis::ArrayItemType::NestedStructRef {
                        properties,
                        ..
                    } = item_type
                    {
                        let scalar_json = |kind: crate::analysis::QueryScalarType| match kind {
                            crate::analysis::QueryScalarType::String => {
                                quote! { ::serde_json::Value::String(value.clone()) }
                            }
                            crate::analysis::QueryScalarType::Boolean => quote! {
                                ::serde_json::Value::Bool(value.parse::<bool>().map_err(|error| {
                                    format!("invalid boolean query value `{value}`: {error}")
                                })?)
                            },
                            crate::analysis::QueryScalarType::Integer
                            | crate::analysis::QueryScalarType::Number => quote! {
                                match ::serde_json::from_str::<::serde_json::Value>(value)
                                    .map_err(|error| format!("invalid numeric query value `{value}`: {error}"))?
                                {
                                    parsed @ ::serde_json::Value::Number(_) => parsed,
                                    _ => return Err(format!("invalid numeric query value `{value}`")),
                                }
                            },
                        };
                        let property_decoders = properties
                            .iter()
                            .enumerate()
                            .map(|(property_index, property)| {
                                let property_name = property.wire_name.as_str();
                                match &property.value_type {
                                    crate::analysis::QueryStructPropertyType::Scalar(kind) => {
                                        let parsed = scalar_json(*kind);
                                        quote! {
                                            for (key, value) in &__pairs {
                                                let Some(rest) = key.strip_prefix(prefix) else { continue };
                                                let Some((index, tail)) = rest.split_once('.') else { continue };
                                                let Ok(index) = index.parse::<usize>() else { continue };
                                                if tail != #property_name { continue; }
                                                let parsed = #parsed;
                                                groups.entry(index).or_default()
                                                    .insert(#property_name.to_string(), parsed);
                                            }
                                        }
                                    }
                                    crate::analysis::QueryStructPropertyType::Object { properties } => {
                                        let nested_groups = format_ident!("nested_objects_{property_index}");
                                        let leaf_parsers = properties.iter().map(|leaf_property| {
                                            let leaf = leaf_property.wire_name.as_str();
                                            let crate::analysis::QueryStructPropertyType::Scalar(kind) = leaf_property.value_type else {
                                                unreachable!("flat query object cannot contain nested values");
                                            };
                                            let parsed = scalar_json(kind);
                                            quote! { #leaf => #parsed, }
                                        }).collect::<Vec<_>>();
                                        quote! {
                                            let mut #nested_groups: ::std::collections::BTreeMap<usize, ::serde_json::Map<String, ::serde_json::Value>> = ::std::collections::BTreeMap::new();
                                            for (key, value) in &__pairs {
                                                let Some(rest) = key.strip_prefix(prefix) else { continue };
                                                let Some((index, tail)) = rest.split_once('.') else { continue };
                                                let Ok(index) = index.parse::<usize>() else { continue };
                                                if tail == concat!(#property_name, "[]") {
                                                    groups.entry(index).or_default().insert(
                                                        #property_name.to_string(),
                                                        ::serde_json::Value::Object(::serde_json::Map::new()),
                                                    );
                                                    continue;
                                                }
                                                let Some(leaf) = tail.strip_prefix(concat!(#property_name, ".")) else { continue };
                                                let parsed = match leaf { #(#leaf_parsers)* _ => continue };
                                                #nested_groups.entry(index).or_default().insert(leaf.to_string(), parsed);
                                            }
                                            for (index, value) in #nested_groups {
                                                groups.entry(index).or_default().insert(#property_name.to_string(), ::serde_json::Value::Object(value));
                                            }
                                        }
                                    }
                                    crate::analysis::QueryStructPropertyType::Array { item_type } => {
                                        let nested_groups = format_ident!("nested_groups_{property_index}");
                                        match item_type {
                                            crate::analysis::ArrayItemType::Scalar(rust_type) => {
                                                let kind = if rust_type == "String" {
                                                    crate::analysis::QueryScalarType::String
                                                } else if rust_type == "bool" {
                                                    crate::analysis::QueryScalarType::Boolean
                                                } else if rust_type.starts_with('i') || rust_type.starts_with('u') {
                                                    crate::analysis::QueryScalarType::Integer
                                                } else {
                                                    crate::analysis::QueryScalarType::Number
                                                };
                                                let parsed = scalar_json(kind);
                                                quote! {
                                                    let mut #nested_groups: ::std::collections::BTreeMap<usize, ::std::collections::BTreeMap<usize, ::serde_json::Value>> = ::std::collections::BTreeMap::new();
                                                    for (key, value) in &__pairs {
                                                        let Some(rest) = key.strip_prefix(prefix) else { continue };
                                                        let Some((index, tail)) = rest.split_once('.') else { continue };
                                                let Ok(index) = index.parse::<usize>() else { continue };
                                                if tail == concat!(#property_name, "[]") {
                                                    groups.entry(index).or_default().insert(
                                                        #property_name.to_string(),
                                                        ::serde_json::Value::Array(Vec::new()),
                                                    );
                                                    continue;
                                                }
                                                let Some(nested_index) = tail.strip_prefix(concat!(#property_name, ".")) else { continue };
                                                        let Ok(nested_index) = nested_index.parse::<usize>() else { continue };
                                                        let parsed = #parsed;
                                                        #nested_groups.entry(index).or_default().insert(nested_index, parsed);
                                                    }
                                                    for (index, values) in #nested_groups {
                                                        groups.entry(index).or_default().insert(
                                                            #property_name.to_string(),
                                                            ::serde_json::Value::Array(values.into_values().collect()),
                                                        );
                                                    }
                                                }
                                            }
                                            crate::analysis::ArrayItemType::SchemaRef(_) => quote! {
                                                let mut #nested_groups: ::std::collections::BTreeMap<usize, ::std::collections::BTreeMap<usize, ::serde_json::Value>> = ::std::collections::BTreeMap::new();
                                                for (key, value) in &__pairs {
                                                    let Some(rest) = key.strip_prefix(prefix) else { continue };
                                                    let Some((index, tail)) = rest.split_once('.') else { continue };
                                                let Ok(index) = index.parse::<usize>() else { continue };
                                                if tail == concat!(#property_name, "[]") {
                                                    groups.entry(index).or_default().insert(
                                                        #property_name.to_string(),
                                                        ::serde_json::Value::Array(Vec::new()),
                                                    );
                                                    continue;
                                                }
                                                let Some(nested_index) = tail.strip_prefix(concat!(#property_name, ".")) else { continue };
                                                    let Ok(nested_index) = nested_index.parse::<usize>() else { continue };
                                                    #nested_groups.entry(index).or_default().insert(
                                                        nested_index,
                                                        ::serde_json::Value::String(value.clone()),
                                                    );
                                                }
                                                for (index, values) in #nested_groups {
                                                    groups.entry(index).or_default().insert(
                                                        #property_name.to_string(),
                                                        ::serde_json::Value::Array(values.into_values().collect()),
                                                    );
                                                }
                                            },
                                            crate::analysis::ArrayItemType::FlatStructRef { properties, .. } => {
                                                let allowed = properties.iter().map(|property| property.wire_name.clone()).collect::<Vec<_>>();
                                                let leaf_kinds = properties.iter().map(|property| match property.value_type {
                                                    crate::analysis::QueryStructPropertyType::Scalar(kind) => kind,
                                                    crate::analysis::QueryStructPropertyType::Array { .. }
                                                    | crate::analysis::QueryStructPropertyType::Object { .. } => unreachable!("flat query struct cannot contain nested values"),
                                                }).collect::<Vec<_>>();
                                                let leaf_parsers = allowed.iter().zip(leaf_kinds).map(|(leaf, kind)| {
                                                    let parsed = scalar_json(kind);
                                                    quote! {
                                                        #leaf => #parsed,
                                                    }
                                                }).collect::<Vec<_>>();
                                                quote! {
                                                    let mut #nested_groups: ::std::collections::BTreeMap<usize, ::std::collections::BTreeMap<usize, ::serde_json::Map<String, ::serde_json::Value>>> = ::std::collections::BTreeMap::new();
                                                    for (key, value) in &__pairs {
                                                        let Some(rest) = key.strip_prefix(prefix) else { continue };
                                                        let Some((index, tail)) = rest.split_once('.') else { continue };
                                                let Ok(index) = index.parse::<usize>() else { continue };
                                                if tail == concat!(#property_name, "[]") {
                                                    groups.entry(index).or_default().insert(
                                                        #property_name.to_string(),
                                                        ::serde_json::Value::Array(Vec::new()),
                                                    );
                                                    continue;
                                                }
                                                let Some(tail) = tail.strip_prefix(concat!(#property_name, ".")) else { continue };
                                                if let Some(nested_index) = tail.strip_suffix("[]").and_then(|index| index.parse::<usize>().ok()) {
                                                    #nested_groups.entry(index).or_default().entry(nested_index).or_default();
                                                    continue;
                                                }
                                                        let Some((nested_index, leaf)) = tail.split_once('.') else { continue };
                                                        let Ok(nested_index) = nested_index.parse::<usize>() else { continue };
                                                        let parsed = match leaf {
                                                            #(#leaf_parsers)*
                                                            _ => continue,
                                                        };
                                                        #nested_groups.entry(index).or_default()
                                                            .entry(nested_index).or_default()
                                                            .insert(leaf.to_string(), parsed);
                                                    }
                                                    for (index, values) in #nested_groups {
                                                        groups.entry(index).or_default().insert(
                                                            #property_name.to_string(),
                                                            ::serde_json::Value::Array(values.into_values().map(::serde_json::Value::Object).collect()),
                                                        );
                                                    }
                                                }
                                            }
                                            crate::analysis::ArrayItemType::NestedStructRef { .. } => unreachable!("analysis rejects query nesting deeper than two levels"),
                                        }
                                    }
                                }
                            })
                            .collect::<Vec<_>>();
                        quote! {
                            let #field_ident = {
                                let empty_marker = __query_empty_marker(&__pairs, #wire_name)?;
                                let prefix = concat!(#wire_name, ".");
                                let mut groups: ::std::collections::BTreeMap<usize, ::serde_json::Map<String, ::serde_json::Value>> = ::std::collections::BTreeMap::new();
                                for (key, _) in &__pairs {
                                    let Some(rest) = key.strip_prefix(prefix) else { continue };
                                    if let Some(index) = rest.strip_suffix("[]").and_then(|index| index.parse::<usize>().ok()) {
                                        groups.entry(index).or_default();
                                    }
                                }
                                #(#property_decoders)*
                                if empty_marker && !groups.is_empty() {
                                    return Err(format!(
                                        "query array `{}` cannot combine values with its empty marker",
                                        #wire_name,
                                    ));
                                }
                                if empty_marker {
                                    Some(Vec::new())
                                } else if groups.is_empty() {
                                    None
                                } else {
                                    let mut values = Vec::with_capacity(groups.len());
                                    for (_, object) in groups {
                                        values.push(
                                            ::serde_json::from_value(::serde_json::Value::Object(object))
                                                .map_err(|error| format!("invalid nested query structure for `{}`: {error}", #wire_name))?,
                                        );
                                    }
                                    Some(values)
                                }
                            };
                        }
                    } else {
                        quote! {
                            let #field_ident = {
                                let empty_marker = __query_empty_marker(&__pairs, #wire_name)?;
                                let raw_values: Vec<&str> = __pairs
                                    .iter()
                                    .filter(|(key, _)| key == #wire_name)
                                    .map(|(_, value)| value.as_str())
                                    .collect();
                                if empty_marker && !raw_values.is_empty() {
                                    return Err(format!(
                                        "query array `{}` cannot combine values with its empty marker",
                                        #wire_name,
                                    ));
                                }
                                if empty_marker {
                                    Some(Vec::new())
                                } else if raw_values.is_empty() {
                                    None
                                } else {
                                    let mut values = Vec::with_capacity(raw_values.len());
                                    for raw in raw_values {
                                        values.push(__decode_query_scalar(raw, #wire_name)?);
                                    }
                                    Some(values)
                                }
                            };
                        }
                    }
                }
                Some(QuerySerialization::FormArray { .. }) => quote! {
                    let #field_ident = match (
                        __query_one(&__pairs, #wire_name)?,
                        __query_empty_marker(&__pairs, #wire_name)?,
                    ) {
                        (Some(_), true) => return Err(format!(
                            "query array `{}` cannot combine a value with its empty marker",
                            #wire_name,
                        )),
                        (Some(raw), false) => {
                            let mut values = Vec::new();
                            for item in raw.split(',') {
                                values.push(__decode_query_scalar(item, #wire_name)?);
                            }
                            Some(values)
                        }
                        (None, true) => Some(Vec::new()),
                        (None, false) => None,
                    };
                },
                Some(QuerySerialization::FormExplodedNestedObject { properties }) => {
                    let scalar_json = |kind: crate::analysis::QueryScalarType| match kind {
                        crate::analysis::QueryScalarType::String => {
                            quote! { ::serde_json::Value::String(value.clone()) }
                        }
                        crate::analysis::QueryScalarType::Boolean => quote! {
                            ::serde_json::Value::Bool(value.parse::<bool>().map_err(|error| format!("invalid boolean query value `{value}`: {error}"))?)
                        },
                        crate::analysis::QueryScalarType::Integer
                        | crate::analysis::QueryScalarType::Number => quote! {
                            match ::serde_json::from_str::<::serde_json::Value>(value)
                                .map_err(|error| format!("invalid numeric query value `{value}`: {error}"))?
                            {
                                parsed @ ::serde_json::Value::Number(_) => parsed,
                                _ => return Err(format!("invalid numeric query value `{value}`")),
                            }
                        },
                    };
                    let property_decoders = properties.iter().enumerate().map(|(property_index, property)| {
                        let property_name = property.wire_name.as_str();
                        match &property.value_type {
                            crate::analysis::QueryStructPropertyType::Scalar(kind) => {
                                let parsed = scalar_json(*kind);
                                quote! {
                                    if let Some((_, value)) = __pairs.iter().find(|(key, _)| key == concat!(#wire_name, ".", #property_name)) {
                                        let parsed = #parsed;
                                        object.insert(#property_name.to_string(), parsed);
                                    }
                                }
                            }
                            crate::analysis::QueryStructPropertyType::Object { properties } => {
                                let property_wire_name = format!("{wire_name}.{property_name}");
                                let leaf_parsers = properties.iter().map(|leaf_property| {
                                    let leaf = leaf_property.wire_name.as_str();
                                    let crate::analysis::QueryStructPropertyType::Scalar(kind) = leaf_property.value_type else {
                                        unreachable!("flat query object cannot contain nested values");
                                    };
                                    let parsed = scalar_json(kind);
                                    quote! { #leaf => #parsed, }
                                }).collect::<Vec<_>>();
                                quote! {
                                    let mut nested = ::serde_json::Map::new();
                                    for (key, value) in &__pairs {
                                        let Some(leaf) = key.strip_prefix(concat!(#wire_name, ".", #property_name, ".")) else { continue };
                                        let parsed = match leaf { #(#leaf_parsers)* _ => continue };
                                        nested.insert(leaf.to_string(), parsed);
                                    }
                                    let nested_empty_marker = __query_empty_marker(&__pairs, #property_wire_name)?;
                                    if nested_empty_marker && !nested.is_empty() {
                                        return Err(format!("query object `{}` cannot combine properties with its empty marker", #property_wire_name));
                                    }
                                    if nested_empty_marker || !nested.is_empty() {
                                        object.insert(#property_name.to_string(), ::serde_json::Value::Object(nested));
                                    }
                                }
                            }
                            crate::analysis::QueryStructPropertyType::Array { item_type } => {
                                let values_ident = format_ident!("property_values_{property_index}");
                                let property_wire_name = format!("{wire_name}.{property_name}");
                                match item_type {
                                    crate::analysis::ArrayItemType::Scalar(rust_type) => {
                                        let kind = if rust_type == "String" {
                                            crate::analysis::QueryScalarType::String
                                        } else if rust_type == "bool" {
                                            crate::analysis::QueryScalarType::Boolean
                                        } else if rust_type.starts_with('i') || rust_type.starts_with('u') {
                                            crate::analysis::QueryScalarType::Integer
                                        } else {
                                            crate::analysis::QueryScalarType::Number
                                        };
                                        let parsed = scalar_json(kind);
                                        quote! {
                                            let mut #values_ident: ::std::collections::BTreeMap<usize, ::serde_json::Value> = ::std::collections::BTreeMap::new();
                                            for (key, value) in &__pairs {
                                                let Some(index) = key.strip_prefix(concat!(#wire_name, ".", #property_name, ".")) else { continue };
                                                let Ok(index) = index.parse::<usize>() else { continue };
                                                let parsed = #parsed;
                                                #values_ident.insert(index, parsed);
                                            }
                                            let property_empty_marker = __query_empty_marker(&__pairs, #property_wire_name)?;
                                            if property_empty_marker && !#values_ident.is_empty() {
                                                return Err(format!("query array `{}` cannot combine values with its empty marker", #property_wire_name));
                                            }
                                            if property_empty_marker || !#values_ident.is_empty() {
                                                object.insert(#property_name.to_string(), ::serde_json::Value::Array(#values_ident.into_values().collect()));
                                            }
                                        }
                                    }
                                    crate::analysis::ArrayItemType::SchemaRef(_) => quote! {
                                        let mut #values_ident: ::std::collections::BTreeMap<usize, ::serde_json::Value> = ::std::collections::BTreeMap::new();
                                        for (key, value) in &__pairs {
                                            let Some(index) = key.strip_prefix(concat!(#wire_name, ".", #property_name, ".")) else { continue };
                                            let Ok(index) = index.parse::<usize>() else { continue };
                                            #values_ident.insert(index, ::serde_json::Value::String(value.clone()));
                                        }
                                        let property_empty_marker = __query_empty_marker(&__pairs, #property_wire_name)?;
                                        if property_empty_marker && !#values_ident.is_empty() {
                                            return Err(format!("query array `{}` cannot combine values with its empty marker", #property_wire_name));
                                        }
                                        if property_empty_marker || !#values_ident.is_empty() {
                                            object.insert(#property_name.to_string(), ::serde_json::Value::Array(#values_ident.into_values().collect()));
                                        }
                                    },
                                    crate::analysis::ArrayItemType::FlatStructRef { properties, .. } => {
                                        let leaf_parsers = properties.iter().map(|leaf_property| {
                                            let leaf = leaf_property.wire_name.as_str();
                                            let crate::analysis::QueryStructPropertyType::Scalar(kind) = leaf_property.value_type else {
                                                unreachable!("flat query struct cannot contain arrays");
                                            };
                                            let parsed = scalar_json(kind);
                                            quote! { #leaf => #parsed, }
                                        }).collect::<Vec<_>>();
                                        quote! {
                                            let mut #values_ident: ::std::collections::BTreeMap<usize, ::serde_json::Map<String, ::serde_json::Value>> = ::std::collections::BTreeMap::new();
                                            for (key, value) in &__pairs {
                                                let Some(tail) = key.strip_prefix(concat!(#wire_name, ".", #property_name, ".")) else { continue };
                                                let Some((index, leaf)) = tail.split_once('.') else { continue };
                                                let Ok(index) = index.parse::<usize>() else { continue };
                                                let parsed = match leaf { #(#leaf_parsers)* _ => continue };
                                                #values_ident.entry(index).or_default().insert(leaf.to_string(), parsed);
                                            }
                                            let property_empty_marker = __query_empty_marker(&__pairs, #property_wire_name)?;
                                            if property_empty_marker && !#values_ident.is_empty() {
                                                return Err(format!("query array `{}` cannot combine values with its empty marker", #property_wire_name));
                                            }
                                            if property_empty_marker || !#values_ident.is_empty() {
                                                object.insert(#property_name.to_string(), ::serde_json::Value::Array(
                                                    #values_ident.into_values().map(::serde_json::Value::Object).collect()
                                                ));
                                            }
                                        }
                                    }
                                    crate::analysis::ArrayItemType::NestedStructRef { .. } => unreachable!("analysis rejects query nesting deeper than two levels"),
                                }
                            }
                        }
                    }).collect::<Vec<_>>();
                    quote! {
                        let #field_ident = {
                            let empty_marker = __query_empty_marker(&__pairs, #wire_name)?;
                            let mut object = ::serde_json::Map::new();
                            #(#property_decoders)*
                            if empty_marker && !object.is_empty() {
                                return Err(format!("query object `{}` cannot combine properties with its empty marker", #wire_name));
                            }
                            if object.is_empty() && !empty_marker {
                                None
                            } else {
                                Some(::serde_json::from_value(::serde_json::Value::Object(object))
                                    .map_err(|error| format!("invalid nested query object for `{}`: {error}", #wire_name))?)
                            }
                        };
                    }
                }
                Some(QuerySerialization::FormExplodedObject) => {
                    let property_names = self
                        .query_object_properties(parameter)
                        .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    let required_names = self.query_object_required_properties(parameter);
                    quote! {
                        let #field_ident = {
                            let empty_marker = __query_empty_marker(&__pairs, #wire_name)?;
                            let allowed = [#(#property_names),*];
                            let object_fields: Vec<(String, String)> = __pairs
                                .iter()
                                .filter(|(key, _)| allowed.contains(&key.as_str()))
                                .cloned()
                                .collect();
                            if empty_marker && !object_fields.is_empty() {
                                return Err(format!(
                                    "query object `{}` cannot combine properties with its empty marker",
                                    #wire_name,
                                ));
                            }
                            if object_fields.is_empty() && !empty_marker {
                                None
                            } else {
                                let required_properties: &[&str] = &[#(#required_names),*];
                                for required in required_properties {
                                    if !object_fields.iter().any(|(key, _)| key == required) {
                                        return Err(format!("query object `{}` is missing a required property", #wire_name));
                                    }
                                }
                                Some(__decode_query_object(&object_fields, #wire_name)?)
                            }
                        };
                    }
                }
                Some(QuerySerialization::FormObject) => {
                    let property_names = self
                        .query_object_properties(parameter)
                        .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    let required_names = self.query_object_required_properties(parameter);
                    let has_required_names = !required_names.is_empty();
                    quote! {
                        let #field_ident = match (
                            __query_one(&__pairs, #wire_name)?,
                            __query_empty_marker(&__pairs, #wire_name)?,
                        ) {
                            (Some(_), true) => return Err(format!(
                                "query object `{}` cannot combine a value with its empty marker",
                                #wire_name,
                            )),
                            (Some(raw), false) => {
                                let parts: Vec<&str> = raw.split(',').collect();
                                if parts.len() % 2 != 0 {
                                    return Err(format!(
                                        "query object `{}` must contain alternating key,value entries",
                                        #wire_name,
                                    ));
                                }
                                let allowed = [#(#property_names),*];
                                let mut seen = ::std::collections::BTreeSet::new();
                                let object_fields: Vec<(String, String)> = parts
                                    .chunks_exact(2)
                                    .map(|pair| (pair[0].to_string(), pair[1].to_string()))
                                    .collect();
                                for (key, _) in &object_fields {
                                    if !allowed.contains(&key.as_str()) || !seen.insert(key.as_str()) {
                                        return Err(format!("query object `{}` has invalid properties", #wire_name));
                                    }
                                }
                                let required_properties: &[&str] = &[#(#required_names),*];
                                for required in required_properties {
                                    if !seen.contains(required) {
                                        return Err(format!("query object `{}` is missing a required property", #wire_name));
                                    }
                                }
                                Some(__decode_query_object(&object_fields, #wire_name)?)
                            }
                            (None, true) if #has_required_names => return Err(format!(
                                "query object `{}` is missing a required property",
                                #wire_name,
                            )),
                            (None, true) => Some(__decode_query_object(&[], #wire_name)?),
                            (None, false) => None,
                        };
                    }
                }
                Some(QuerySerialization::DeepObject) => {
                    let property_names = self
                        .query_object_properties(parameter)
                        .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
                        .unwrap_or_default();
                    let required_names = self.query_object_required_properties(parameter);
                    quote! {
                        let #field_ident = {
                            let empty_marker = __query_empty_marker(&__pairs, #wire_name)?;
                            let prefix = format!("{}[", #wire_name);
                            let allowed = [#(#property_names),*];
                            let mut object_fields = Vec::new();
                            for (key, value) in &__pairs {
                                if let Some(property) = key
                                    .strip_prefix(&prefix)
                                    .and_then(|rest| rest.strip_suffix(']'))
                                {
                                    if property.is_empty() {
                                        continue;
                                    }
                                    if !allowed.contains(&property) {
                                        return Err(format!(
                                            "unknown deepObject property `{}[{}]`",
                                            #wire_name,
                                            property,
                                        ));
                                    }
                                    object_fields.push((property.to_string(), value.clone()));
                                }
                            }
                            if empty_marker && !object_fields.is_empty() {
                                return Err(format!(
                                    "query object `{}` cannot combine properties with its empty marker",
                                    #wire_name,
                                ));
                            }
                            if object_fields.is_empty() && !empty_marker {
                                None
                            } else {
                                let required_properties: &[&str] = &[#(#required_names),*];
                                for required in required_properties {
                                    if !object_fields.iter().any(|(key, _)| key == required) {
                                        return Err(format!("query object `{}` is missing a required property", #wire_name));
                                    }
                                }
                                Some(__decode_query_object(&object_fields, #wire_name)?)
                            }
                        };
                    }
                }
                Some(
                    QuerySerialization::Unsupported { .. }
                    | QuerySerialization::SimpleHeaderArray { .. },
                )
                | None => quote! {
                    let #field_ident = __query_one(&__pairs, #wire_name)?
                        .map(|raw| __decode_query_scalar(&raw, #wire_name))
                        .transpose()?;
                },
            };
            decoders.push(decoder);
        }
        let doc = format!(
            " Query parameters for `{} {}` (operationId `{}`).",
            op.method, op.path, op.operation_id
        );
        Some(quote! {
            #[doc = #doc]
            #[derive(Debug, Default)]
            pub struct #ident {
                #(#fields),*
            }

            fn #decode_ident(
                raw: ::std::option::Option<&str>,
            ) -> ::std::result::Result<#ident, String> {
                if let Some(raw) = raw {
                    __validate_urlencoded(raw)?;
                }
                let __pairs = __query_pairs(raw);
                #(#decoders)*
                Ok(#ident {
                    #(#field_idents),*
                })
            }
        })
    }

    fn emit_errors(
        &self,
        ops: &[&OperationInfo],
        validation_enabled: bool,
        response_enum_names: &BTreeMap<String, syn::Ident>,
    ) -> TokenStream {
        let provenance_attribute = self.provenance_attribute();
        let any_streaming = ops.iter().any(|op| op.supports_streaming);
        let enums: Vec<TokenStream> = ops
            .iter()
            .map(|op| self.emit_response_enum(op, response_enum_names))
            .collect();
        let problem_types = validation_enabled.then(|| {
            quote! {
                /// RFC 9457 Problem Details profile used for rejected requests.
                #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
                pub struct ProblemDetails {
                    #[serde(rename = "type")]
                    pub r#type: String,
                    pub title: String,
                    pub status: u16,
                    pub code: String,
                    #[serde(default, skip_serializing_if = "Vec::is_empty")]
                    pub errors: Vec<InvalidParameter>,
                }

                /// One sanitized request-contract violation.
                #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
                pub struct InvalidParameter {
                    pub code: String,
                    pub location: String,
                    pub message: String,
                }

                /// Axum rejection wrapper which always uses `application/problem+json`.
                #[derive(Debug, Clone)]
                pub struct RequestValidationRejection(pub ProblemDetails);

                impl IntoResponse for RequestValidationRejection {
                    fn into_response(self) -> ::axum::response::Response {
                        let status = StatusCode::from_u16(self.0.status)
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                        let mut response = (status, Json(self.0)).into_response();
                        response.headers_mut().insert(
                            ::axum::http::header::CONTENT_TYPE,
                            ::axum::http::HeaderValue::from_static("application/problem+json"),
                        );
                        response
                    }
                }
            }
        });

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

            #provenance_attribute

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

            #problem_types

            #stream_alias

            #(#enums)*
        }
    }

    fn emit_response_enum(
        &self,
        op: &OperationInfo,
        response_enum_names: &BTreeMap<String, syn::Ident>,
    ) -> TokenStream {
        let enum_ident = &response_enum_names[&op.operation_id];
        let mut variants: Vec<TokenStream> = Vec::new();
        let mut arms: Vec<TokenStream> = Vec::new();

        // Analyses produced before complete response metadata existed (and a
        // few unit tests that construct OperationInfo directly) still expose
        // `response_schemas`. Keep that compatibility path while treating the
        // complete response map as authoritative for real specs.
        let fallback_responses;
        let responses = if let Some(responses) = self
            .analysis
            .operation_responses
            .get(&op.operation_id)
            .filter(|responses| !responses.is_empty())
        {
            responses
        } else {
            fallback_responses = op
                .response_schemas
                .iter()
                .map(|(status, schema_name)| {
                    (
                        status.clone(),
                        OperationResponse {
                            schema_name: Some(schema_name.clone()),
                            media_type: Some("application/json".to_string()),
                            body: Some(OperationResponseBody::Json {
                                schema_name: schema_name.clone(),
                                media_type: "application/json".to_string(),
                            }),
                            supports_streaming: false,
                            has_content: true,
                            unsupported_media_types: Vec::new(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>();
            &fallback_responses
        };

        for (status, response) in responses {
            let base_name = status_variant_name(status);
            let variant = format_ident!("{}", base_name);
            let runtime_status = response_uses_runtime_status(status);
            let status_expr = status_token(status);
            let status_guard = runtime_status_guard(status, quote! { status });

            let buffered_body = response.body.clone().or_else(|| {
                response
                    .schema_name
                    .as_ref()
                    .map(|schema_name| OperationResponseBody::Json {
                        schema_name: schema_name.clone(),
                        media_type: response
                            .media_type
                            .clone()
                            .unwrap_or_else(|| "application/json".to_string()),
                    })
            });

            if let Some(body) = buffered_body {
                let (body_ty, response_body, media_type, wildcard) = match body {
                    OperationResponseBody::Json {
                        schema_name,
                        media_type,
                    } => (
                        self.model_type(&schema_name),
                        quote! { Json(body) },
                        media_type,
                        false,
                    ),
                    OperationResponseBody::Text { media_type } => {
                        (quote! { String }, quote! { body }, media_type, false)
                    }
                    OperationResponseBody::Binary {
                        media_type,
                        wildcard,
                    } => (
                        quote! { bytes::Bytes },
                        quote! { body },
                        media_type,
                        wildcard,
                    ),
                };
                if wildcard {
                    if runtime_status {
                        variants.push(quote! {
                            #variant(StatusCode, ::axum::http::HeaderValue, #body_ty)
                        });
                        arms.push(quote! {
                            Self::#variant(status, content_type, body) => {
                                if !(#status_guard) {
                                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                                }
                                let mut response = (status, #response_body).into_response();
                                response.headers_mut().insert(
                                    ::axum::http::header::CONTENT_TYPE,
                                    content_type,
                                );
                                response
                            }
                        });
                    } else {
                        variants.push(quote! {
                            #variant(::axum::http::HeaderValue, #body_ty)
                        });
                        arms.push(quote! {
                            Self::#variant(content_type, body) => {
                                let mut response = (#status_expr, #response_body).into_response();
                                response.headers_mut().insert(
                                    ::axum::http::header::CONTENT_TYPE,
                                    content_type,
                                );
                                response
                            }
                        });
                    }
                } else if runtime_status {
                    variants.push(quote! { #variant(StatusCode, #body_ty) });
                    arms.push(quote! {
                        Self::#variant(status, body) => {
                            if !(#status_guard) {
                                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                            }
                            let mut response = (status, #response_body).into_response();
                            let Ok(content_type) = ::axum::http::HeaderValue::from_bytes(#media_type.as_bytes()) else {
                                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                            };
                            response.headers_mut().insert(
                                ::axum::http::header::CONTENT_TYPE,
                                content_type,
                            );
                            response
                        }
                    });
                } else {
                    variants.push(quote! { #variant(#body_ty) });
                    arms.push(quote! {
                        Self::#variant(body) => {
                            let mut response = (#status_expr, #response_body).into_response();
                            let Ok(content_type) = ::axum::http::HeaderValue::from_bytes(#media_type.as_bytes()) else {
                                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                            };
                            response.headers_mut().insert(
                                ::axum::http::header::CONTENT_TYPE,
                                content_type,
                            );
                            response
                        }
                    });
                }
            } else if !response.has_content {
                if runtime_status {
                    variants.push(quote! { #variant(StatusCode) });
                    arms.push(quote! {
                        Self::#variant(status) => {
                            if #status_guard {
                                status.into_response()
                            } else {
                                StatusCode::INTERNAL_SERVER_ERROR.into_response()
                            }
                        }
                    });
                } else {
                    variants.push(quote! { #variant });
                    arms.push(quote! {
                        Self::#variant => #status_expr.into_response()
                    });
                }
            }

            // The stream variant belongs to the response which declared SSE,
            // so it carries that response's status instead of implicitly 200.
            if response.supports_streaming {
                let stream_variant = format_ident!("{}Stream", base_name);
                if runtime_status {
                    variants.push(quote! {
                        #stream_variant(StatusCode, ::axum::response::sse::Sse<ServerEventStream>)
                    });
                    arms.push(quote! {
                        Self::#stream_variant(status, sse) => {
                            if !(#status_guard) {
                                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                            }
                            let mut response = sse.into_response();
                            *response.status_mut() = status;
                            response
                        }
                    });
                } else {
                    variants.push(quote! {
                        #stream_variant(::axum::response::sse::Sse<ServerEventStream>)
                    });
                    arms.push(quote! {
                        Self::#stream_variant(sse) => {
                            let mut response = sse.into_response();
                            *response.status_mut() = #status_expr;
                            response
                        }
                    });
                }
            }
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

fn parameter_location(location: &str, name: &str) -> String {
    format!("/{location}/{}", name.replace('~', "~0").replace('/', "~1"))
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

fn path_parameter_affixes<'a>(path: &'a str, parameter_name: &str) -> Option<(&'a str, &'a str)> {
    let marker = format!("{{{parameter_name}}}");
    for segment in path.split('/') {
        let Some(start) = segment.find(&marker) else {
            continue;
        };
        let prefix = &segment[..start];
        let suffix = &segment[start + marker.len()..];
        if prefix.is_empty() && suffix.is_empty() {
            return None;
        }
        return Some((prefix, suffix));
    }
    None
}

/// Validate and convert an OpenAPI path template into Axum 0.8 route syntax.
///
/// Both formats use `{parameter}` for a dynamic segment. Axum only supports a
/// capture as a complete segment, so OpenAPI templates embedded in a literal
/// segment are rejected during generation instead of panicking when the
/// generated router is constructed.
fn openapi_to_axum_path(path: &str) -> Result<String, ServerCodegenError> {
    let invalid = |reason: &str| ServerCodegenError::InvalidRoutePath {
        path: path.to_string(),
        reason: reason.to_string(),
    };

    if !path.starts_with('/') {
        return Err(invalid("paths must start with `/`"));
    }
    let mut route_segments = Vec::new();
    for segment in path.split('/').skip(1) {
        if segment.is_empty() {
            route_segments.push(String::new());
            continue;
        }
        if segment.starts_with(':') || segment.starts_with('*') {
            return Err(invalid(
                "segments beginning with `:` or `*` conflict with Axum route syntax",
            ));
        }

        let has_open = segment.contains('{');
        let has_close = segment.contains('}');
        if has_open || has_close {
            let Some(open) = segment.find('{') else {
                return Err(invalid(
                    "path parameter has a closing brace without an opening brace",
                ));
            };
            let Some(relative_close) = segment[open + 1..].find('}') else {
                return Err(invalid(
                    "path parameter has an opening brace without a closing brace",
                ));
            };
            let close = open + 1 + relative_close;
            let name = &segment[open + 1..close];
            let prefix = &segment[..open];
            let suffix = &segment[close + 1..];
            if name.is_empty() || name.contains(['{', '}']) {
                return Err(invalid(
                    "path parameter names must be non-empty and cannot contain braces",
                ));
            }
            if prefix.contains(['{', '}']) || suffix.contains(['{', '}']) {
                return Err(invalid(
                    "embedded path segments may contain exactly one parameter",
                ));
            }
            route_segments.push(format!("{{{name}}}"));
        } else {
            route_segments.push(segment.to_string());
        }
    }

    Ok(format!("/{}", route_segments.join("/")))
}

fn body_type(op: &OperationInfo) -> Option<String> {
    match &op.request_body {
        Some(RequestBodyContent::Json { schema_name, .. })
        | Some(RequestBodyContent::FormUrlEncoded { schema_name, .. })
        | Some(RequestBodyContent::Multipart { schema_name, .. }) => Some(schema_name.clone()),
        Some(RequestBodyContent::OctetStream { .. } | RequestBodyContent::Binary { .. }) => {
            Some("bytes::Bytes".to_string())
        }
        Some(RequestBodyContent::TextPlain { .. }) => Some("String".to_string()),
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

/// Reject distinct raw primary tags which would emit the same Rust items.
/// Sorting the raw tags first keeps the selected pair and diagnostic stable
/// even when selectors are reordered in configuration.
fn validate_tag_identifier_collisions(ops: &[&OperationInfo]) -> Result<(), ServerCodegenError> {
    let raw_tags: std::collections::BTreeSet<String> =
        ops.iter().map(|operation| primary_tag(operation)).collect();
    let mut raw_by_identifier: BTreeMap<String, String> = BTreeMap::new();
    for raw_tag in raw_tags {
        let identifier = trait_ident_for_tag(&raw_tag).to_string();
        if let Some(first_tag) = raw_by_identifier.insert(identifier.clone(), raw_tag.clone()) {
            return Err(ServerCodegenError::TagIdentifierCollision {
                first_tag,
                second_tag: raw_tag,
                identifier,
            });
        }
    }
    Ok(())
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

fn canonical_axum_route_shape(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with('}') {
                "{}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn validate_normalized_route_collisions(ops: &[&OperationInfo]) -> Result<(), ServerCodegenError> {
    let mut seen: BTreeMap<(String, String), &str> = BTreeMap::new();
    for op in ops {
        let normalized = openapi_to_axum_path(&op.path)?;
        let key = (
            canonical_axum_route_shape(&normalized),
            op.method.to_ascii_uppercase(),
        );
        if let Some(previous) = seen.insert(key, &op.path)
            && previous != op.path
        {
            return Err(ServerCodegenError::InvalidRoutePath {
                path: op.path.clone(),
                reason: format!(
                    "normalizes to the same Axum route and method as `{previous}`; embedded path affixes must remain unambiguous"
                ),
            });
        }
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
/// Rust response-enum variant name for an OpenAPI response key.
pub fn status_variant_name(status: &str) -> String {
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
        "1XX" => "Informational".into(),
        "2XX" => "Success".into(),
        "3XX" => "Redirection".into(),
        "4XX" => "ClientError".into(),
        "5XX" => "ServerError".into(),
        other => format!("Status{}", other.to_ascii_uppercase().replace('X', "x")),
    }
}

fn response_uses_runtime_status(status: &str) -> bool {
    status == "default" || matches!(status.as_bytes(), [b'1'..=b'5', b'X' | b'x', b'X' | b'x'])
}

fn runtime_status_guard(status: &str, value: TokenStream) -> TokenStream {
    if status == "default" {
        quote! { true }
    } else {
        let class = u16::from(
            status
                .as_bytes()
                .first()
                .map(|digit| digit - b'0')
                .unwrap_or_default(),
        );
        quote! { #value.as_u16() / 100 == #class }
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
        // Range/default responses carry a runtime StatusCode in their enum
        // variant and never reach this fixed-status helper.
        "1XX" | "2XX" | "3XX" | "4XX" | "5XX" => {
            quote! { StatusCode::INTERNAL_SERVER_ERROR }
        }
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
    fn embedded_path_affix_route_collisions_are_rejected() {
        let json = OperationInfo {
            operation_id: "json".into(),
            method: "get".into(),
            path: "/specs/{id}.json".into(),
            ..Default::default()
        };
        let yaml = OperationInfo {
            operation_id: "yaml".into(),
            method: "get".into(),
            path: "/specs/{name}.yaml".into(),
            ..Default::default()
        };
        let error = validate_normalized_route_collisions(&[&json, &yaml]).unwrap_err();
        assert!(error.to_string().contains("same Axum route"), "{error}");
    }

    #[test]
    fn untagged_falls_back_to_server_api() {
        let id = trait_ident_for_tag("");
        assert_eq!(id.to_string(), "ServerApi");
    }

    #[test]
    fn colliding_raw_tags_are_rejected_in_stable_order() {
        let first = OperationInfo {
            operation_id: "first".into(),
            tags: vec!["foo_bar".into()],
            ..Default::default()
        };
        let second = OperationInfo {
            operation_id: "second".into(),
            tags: vec!["foo-bar".into()],
            ..Default::default()
        };
        let error = validate_tag_identifier_collisions(&[&first, &second]).unwrap_err();
        assert!(matches!(
            error,
            ServerCodegenError::TagIdentifierCollision {
                first_tag,
                second_tag,
                identifier,
            } if first_tag == "foo-bar"
                && second_tag == "foo_bar"
                && identifier == "FooBarApi"
        ));
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

    #[test]
    fn openapi_parameterized_paths_are_axum_08_paths() {
        assert_eq!(
            openapi_to_axum_path("/pets/{pet_id}").unwrap(),
            "/pets/{pet_id}"
        );
        assert_eq!(openapi_to_axum_path("/").unwrap(), "/");
        assert_eq!(
            openapi_to_axum_path("/specs/{provider}/{api}.json").unwrap(),
            "/specs/{provider}/{api}"
        );
    }

    #[test]
    fn embedded_path_parameter_affixes_are_preserved_for_extraction() {
        assert_eq!(
            path_parameter_affixes("/specs/{provider}/{api}.json", "api"),
            Some(("", ".json"))
        );
        assert_eq!(
            path_parameter_affixes("/{provider}.json", "provider"),
            Some(("", ".json"))
        );
        assert_eq!(path_parameter_affixes("/pets/{id}", "id"), None);
    }

    #[test]
    fn malformed_or_unsupported_route_templates_are_rejected() {
        for path in [
            "pets/{pet_id}",
            "/pets/{first}-{second}",
            "/pets/{}",
            "/pets/:id",
        ] {
            assert!(
                matches!(
                    openapi_to_axum_path(path),
                    Err(ServerCodegenError::InvalidRoutePath { .. })
                ),
                "{path} should be rejected"
            );
        }
    }
}
