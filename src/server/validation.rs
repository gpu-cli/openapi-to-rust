use crate::analysis::{OperationInfo, RequestBodyContent, ValidationContext};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde_json::{Map, Value, json};
use std::collections::{BTreeSet, VecDeque};

const BUNDLE_ID: &str = "urn:openapi-to-rust:request-validation";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValidationDraft {
    Draft4,
    Draft202012,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct ValidationTarget {
    pub(crate) operation_id: String,
    pub(crate) location: String,
    pub(crate) parameter_name: Option<String>,
    pub(crate) constant: String,
    pub(crate) pointer: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ValidationBundle {
    pub(crate) document_json: String,
    pub(crate) draft: ValidationDraft,
    pub(crate) targets: Vec<ValidationTarget>,
    pub(crate) has_form_body: bool,
}

impl ValidationBundle {
    #[allow(dead_code)]
    pub(crate) fn target_for(
        &self,
        operation_id: &str,
        location: &str,
        parameter_name: Option<&str>,
    ) -> Option<&ValidationTarget> {
        self.targets.iter().find(|target| {
            target.operation_id == operation_id
                && target.location == location
                && target.parameter_name.as_deref() == parameter_name
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ValidationPreparationError {
    #[error("unsupported OpenAPI version `{0}` for request validation")]
    UnsupportedOpenApiVersion(String),
    #[error("unsupported JSON Schema dialect `{0}` for offline request validation")]
    UnsupportedDialect(String),
    #[error("validation schema `{context}` references unsupported external resource `{reference}`")]
    UnsupportedReference { context: String, reference: String },
    #[error("validation schema `{context}` references missing component schema `{schema}`")]
    MissingComponent { context: String, schema: String },
    #[error("failed to serialize the embedded validation schema: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("embedded validation schemas do not compile offline: {0}")]
    Compile(String),
}

pub(crate) fn prepare_validation_bundle(
    context: &ValidationContext,
    operations: &[&OperationInfo],
) -> Result<ValidationBundle, ValidationPreparationError> {
    let draft = if context.openapi_version.starts_with("3.0.") {
        ValidationDraft::Draft4
    } else if context.openapi_version.starts_with("3.1.")
        || context.openapi_version.starts_with("3.2.")
    {
        ValidationDraft::Draft202012
    } else {
        return Err(ValidationPreparationError::UnsupportedOpenApiVersion(
            context.openapi_version.clone(),
        ));
    };
    if let Some(dialect) = &context.json_schema_dialect {
        let supported = draft == ValidationDraft::Draft202012
            && matches!(
                dialect.as_str(),
                "https://json-schema.org/draft/2020-12/schema"
                    | "https://json-schema.org/draft/2020-12/schema#"
                    | "https://spec.openapis.org/oas/3.1/dialect/base"
                    | "https://spec.openapis.org/oas/3.2/dialect/base"
            );
        if !supported {
            return Err(ValidationPreparationError::UnsupportedDialect(
                dialect.clone(),
            ));
        }
    }
    let definitions_key = match draft {
        ValidationDraft::Draft4 => "definitions",
        ValidationDraft::Draft202012 => "$defs",
    };

    let mut target_values = Map::new();
    let mut targets = Vec::new();
    let mut component_queue = VecDeque::new();
    let mut queued_components = BTreeSet::new();
    let mut target_index = 0usize;

    for operation in operations {
        if let Some(body) = &operation.request_body {
            let schema = match body {
                RequestBodyContent::Json {
                    validation_schema, ..
                }
                | RequestBodyContent::FormUrlEncoded {
                    validation_schema, ..
                } => Some(validation_schema),
                _ => None,
            };
            if let Some(schema) = schema {
                add_target(
                    &operation.operation_id,
                    "BODY",
                    "body",
                    None,
                    schema,
                    draft,
                    definitions_key,
                    &mut target_index,
                    &mut target_values,
                    &mut targets,
                    &mut component_queue,
                    &mut queued_components,
                )?;
            }
        }
        for (parameter_index, parameter) in operation.parameters.iter().enumerate() {
            if let Some(schema) = &parameter.validation_schema {
                add_target(
                    &operation.operation_id,
                    &format!("{}_{}", parameter.location.to_uppercase(), parameter_index),
                    &parameter.location,
                    Some(&parameter.name),
                    schema,
                    draft,
                    definitions_key,
                    &mut target_index,
                    &mut target_values,
                    &mut targets,
                    &mut component_queue,
                    &mut queued_components,
                )?;
            }
        }
    }

    let mut components = Map::new();
    while let Some(name) = component_queue.pop_front() {
        let raw = context.component_schemas.get(&name).ok_or_else(|| {
            ValidationPreparationError::MissingComponent {
                context: "component closure".to_string(),
                schema: name.clone(),
            }
        })?;
        let mut schema = normalize_schema(raw, draft);
        rewrite_references(
            &mut schema,
            &format!("component {name}"),
            definitions_key,
            &mut component_queue,
            &mut queued_components,
        )?;
        components.insert(name, schema);
    }

    let mut definitions = Map::new();
    definitions.insert("targets".to_string(), Value::Object(target_values));
    definitions.insert("components".to_string(), Value::Object(components));
    let mut root = Map::new();
    match draft {
        ValidationDraft::Draft4 => {
            root.insert("id".to_string(), Value::String(BUNDLE_ID.to_string()));
            root.insert(
                "$schema".to_string(),
                Value::String("http://json-schema.org/draft-04/schema#".to_string()),
            );
        }
        ValidationDraft::Draft202012 => {
            root.insert("$id".to_string(), Value::String(BUNDLE_ID.to_string()));
            root.insert(
                "$schema".to_string(),
                Value::String("https://json-schema.org/draft/2020-12/schema".to_string()),
            );
        }
    }
    root.insert(definitions_key.to_string(), Value::Object(definitions));
    let document = Value::Object(root);

    let options = jsonschema::options()
        .with_draft(match draft {
            ValidationDraft::Draft4 => jsonschema::Draft::Draft4,
            ValidationDraft::Draft202012 => jsonschema::Draft::Draft202012,
        })
        .should_validate_formats(true)
        .with_pattern_options(jsonschema::PatternOptions::regex());
    let validators = options
        .build_map(&document)
        .map_err(|error| ValidationPreparationError::Compile(error.to_string()))?;
    for target in &targets {
        if !validators.contains_key(&target.pointer) {
            return Err(ValidationPreparationError::Compile(format!(
                "validator target `{}` was not compiled",
                target.pointer
            )));
        }
    }

    Ok(ValidationBundle {
        document_json: serde_json::to_string(&document)?,
        draft,
        targets,
        has_form_body: operations.iter().any(|operation| {
            matches!(
                operation.request_body,
                Some(RequestBodyContent::FormUrlEncoded { .. })
            )
        }),
    })
}

#[allow(clippy::too_many_arguments)]
fn add_target(
    operation_id: &str,
    suffix: &str,
    location: &str,
    parameter_name: Option<&str>,
    raw_schema: &Value,
    draft: ValidationDraft,
    definitions_key: &str,
    target_index: &mut usize,
    target_values: &mut Map<String, Value>,
    targets: &mut Vec<ValidationTarget>,
    component_queue: &mut VecDeque<String>,
    queued_components: &mut BTreeSet<String>,
) -> Result<(), ValidationPreparationError> {
    let key = format!("v{}", *target_index);
    let mut schema = normalize_schema(raw_schema, draft);
    rewrite_references(
        &mut schema,
        &format!("operation {operation_id} {suffix}"),
        definitions_key,
        component_queue,
        queued_components,
    )?;
    // Keep the target schema at the address exported to generated handlers.
    // `ValidatorMap` may optimize away an otherwise redundant `allOf` wrapper,
    // which would make a constrained inline schema look indistinguishable from
    // an intentionally empty (always-valid) schema.
    target_values.insert(key.clone(), schema);
    targets.push(ValidationTarget {
        operation_id: operation_id.to_string(),
        location: location.to_string(),
        parameter_name: parameter_name.map(str::to_string),
        constant: format!("VALIDATION_TARGET_{}_{}", *target_index, suffix),
        pointer: format!("#/{definitions_key}/targets/{key}"),
    });
    *target_index += 1;
    Ok(())
}

fn normalize_schema(value: &Value, draft: ValidationDraft) -> Value {
    let Value::Object(source) = value else {
        return value.clone();
    };
    let mut schema = source.clone();
    for key in [
        "items",
        "additionalProperties",
        "propertyNames",
        "unevaluatedProperties",
        "unevaluatedItems",
        "contains",
        "contentSchema",
        "if",
        "then",
        "else",
        "not",
    ] {
        if let Some(child) = schema.get_mut(key)
            && child.is_object()
        {
            *child = normalize_schema(child, draft);
        }
    }
    for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
        if let Some(Value::Array(children)) = schema.get_mut(key) {
            for child in children {
                *child = normalize_schema(child, draft);
            }
        }
    }
    for key in [
        "properties",
        "patternProperties",
        "dependentSchemas",
        "$defs",
        "definitions",
    ] {
        if let Some(Value::Object(children)) = schema.get_mut(key) {
            for child in children.values_mut() {
                *child = normalize_schema(child, draft);
            }
        }
    }

    if draft == ValidationDraft::Draft4 {
        let nullable = schema
            .remove("nullable")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if nullable {
            return json!({
                "anyOf": [Value::Object(schema), { "type": "null" }]
            });
        }
    }
    Value::Object(schema)
}

fn rewrite_references(
    value: &mut Value,
    context: &str,
    definitions_key: &str,
    component_queue: &mut VecDeque<String>,
    queued_components: &mut BTreeSet<String>,
) -> Result<(), ValidationPreparationError> {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if let Some(token) = reference.strip_prefix("#/components/schemas/") {
                    let name = unescape_pointer_token(token);
                    if queued_components.insert(name.clone()) {
                        component_queue.push_back(name.clone());
                    }
                    *reference = format!(
                        "{BUNDLE_ID}#/{definitions_key}/components/{}",
                        escape_pointer_token(&name)
                    );
                } else if reference.starts_with(BUNDLE_ID) || !reference.starts_with('#') {
                    return Err(ValidationPreparationError::UnsupportedReference {
                        context: context.to_string(),
                        reference: reference.clone(),
                    });
                }
            }
            for key in [
                "items",
                "additionalProperties",
                "propertyNames",
                "unevaluatedProperties",
                "unevaluatedItems",
                "contains",
                "contentSchema",
                "if",
                "then",
                "else",
                "not",
            ] {
                if let Some(child) = map.get_mut(key) {
                    rewrite_references(
                        child,
                        context,
                        definitions_key,
                        component_queue,
                        queued_components,
                    )?;
                }
            }
            for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
                if let Some(Value::Array(children)) = map.get_mut(key) {
                    for child in children {
                        rewrite_references(
                            child,
                            context,
                            definitions_key,
                            component_queue,
                            queued_components,
                        )?;
                    }
                }
            }
            for key in [
                "properties",
                "patternProperties",
                "dependentSchemas",
                "$defs",
                "definitions",
            ] {
                if let Some(Value::Object(children)) = map.get_mut(key) {
                    for child in children.values_mut() {
                        rewrite_references(
                            child,
                            context,
                            definitions_key,
                            component_queue,
                            queued_components,
                        )?;
                    }
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                rewrite_references(
                    child,
                    context,
                    definitions_key,
                    component_queue,
                    queued_components,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn escape_pointer_token(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn unescape_pointer_token(value: &str) -> String {
    value.replace("~1", "/").replace("~0", "~")
}

pub(crate) fn emit_validation_module(bundle: &ValidationBundle, max_errors: usize) -> TokenStream {
    let document = &bundle.document_json;
    let draft = match bundle.draft {
        ValidationDraft::Draft4 => quote! { ::jsonschema::Draft::Draft4 },
        ValidationDraft::Draft202012 => quote! { ::jsonschema::Draft::Draft202012 },
    };
    let constants = bundle.targets.iter().map(|target| {
        let ident = format_ident!("{}", target.constant);
        let pointer = &target.pointer;
        quote! { pub(crate) const #ident: &str = #pointer; }
    });
    let form_decoder = bundle.has_form_body.then(|| {
        quote! {
            pub(crate) async fn decode_form_body<T>(
                request: ::axum::extract::Request,
                target: &str,
                expected_media_type: &str,
                required: bool,
            max_body_bytes: usize,
            allowed_fields: &[&str],
            required_fields: &[&str],
            ) -> ::std::result::Result<::std::option::Option<T>, RequestValidationRejection>
            where
                T: ::serde::de::DeserializeOwned + ::serde::Serialize,
            {
                let (parts, body) = request.into_parts();
                let content_type = parts.headers.get(::axum::http::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok());
                if !content_type.is_some_and(|value| media_type_is(value, expected_media_type)) {
                    if content_type.is_none() && !required {
                    let bytes = read_body(body, max_body_bytes).await?;
                        if bytes.is_empty() { return Ok(None); }
                    }
                    return Err(unsupported_media_type());
                }
            let bytes = read_body(body, max_body_bytes).await?;
                if bytes.is_empty() {
                    return if required { Err(missing_parameter("/body")) } else { Ok(None) };
                }
                let raw = ::std::str::from_utf8(&bytes).map_err(|_| malformed_parameter("/body"))?;
                validate_urlencoded(raw).map_err(|_| malformed_parameter("/body"))?;
                let mut seen = ::std::collections::BTreeSet::new();
            for (key, _) in ::url::form_urlencoded::parse(raw.as_bytes()) {
                    if !allowed_fields.contains(&key.as_ref()) {
                        return Err(schema_parameter_problem(
                            "/body", "additional_properties", "contains unsupported properties",
                        ));
                    }
                if !seen.insert(key.into_owned()) { return Err(malformed_parameter("/body")); }
            }
            for required_field in required_fields {
                if !seen.contains(*required_field) {
                    return Err(missing_parameter(&format!(
                        "/body/{}", escape_pointer_token(required_field)
                    )));
                }
            }
                let typed: T = ::serde_urlencoded::from_bytes(&bytes).map_err(|_| {
                    schema_parameter_problem("/body", "invalid_value", "is invalid")
                })?;
                validate_parameter(target, "/body", &typed)?;
                Ok(Some(typed))
            }

            fn validate_urlencoded(raw: &str) -> ::std::result::Result<(), ()> {
                let bytes = raw.as_bytes();
                let mut index = 0;
                while index < bytes.len() {
                    if bytes[index] == b'%' {
                        if index + 2 >= bytes.len()
                            || !bytes[index + 1].is_ascii_hexdigit()
                            || !bytes[index + 2].is_ascii_hexdigit()
                        { return Err(()); }
                        index += 3;
                    } else { index += 1; }
                }
                Ok(())
            }
        }
    });

    quote! {
        //! Offline request-schema validators and normalized public rejections.
        #![allow(dead_code)]

        use super::errors::{InvalidParameter, ProblemDetails, RequestValidationRejection};

        const VALIDATION_SCHEMA: &str = #document;
        const MAX_VALIDATION_ERRORS: usize = #max_errors;
        #(#constants)*

        static VALIDATORS: ::std::sync::LazyLock<
            ::std::result::Result<::jsonschema::ValidatorMap, ()>
        > = ::std::sync::LazyLock::new(|| {
            let schema: ::serde_json::Value =
                ::serde_json::from_str(VALIDATION_SCHEMA).map_err(|_| ())?;
            ::jsonschema::options()
                .with_draft(#draft)
                .should_validate_formats(true)
                .with_pattern_options(::jsonschema::PatternOptions::regex())
                .build_map(&schema)
                .map_err(|_| ())
        });

        pub(crate) async fn decode_json_body<T>(
            request: ::axum::extract::Request,
            target: &str,
            expected_media_type: &str,
            required: bool,
            max_body_bytes: usize,
        ) -> ::std::result::Result<::std::option::Option<T>, RequestValidationRejection>
        where
            T: ::serde::de::DeserializeOwned,
        {
            let (parts, body) = request.into_parts();
            let content_type = parts
                .headers
                .get(::axum::http::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok());
            let is_json = content_type
                .is_some_and(|value| media_type_is(value, expected_media_type));

            if !is_json {
                if content_type.is_none() && !required {
                    let bytes = read_body(body, max_body_bytes).await?;
                    if bytes.is_empty() {
                        return Ok(None);
                    }
                }
                return Err(unsupported_media_type());
            }

            let bytes = read_body(body, max_body_bytes).await?;
            if bytes.is_empty() {
                return if required {
                    Err(malformed_request())
                } else {
                    Ok(None)
                };
            }
            let instance: ::serde_json::Value =
                ::serde_json::from_slice(&bytes).map_err(|_| malformed_request())?;
            validate(target, "/body", &instance)?;
            let typed = ::serde_json::from_value(instance)
                .map_err(|_| generated_contract_error())?;
            Ok(Some(typed))
        }

        #form_decoder

        pub(crate) fn decode_parameter<T>(
            raw: &str,
            target: &str,
            location: &str,
            string_wire: bool,
        ) -> ::std::result::Result<T, RequestValidationRejection>
        where
            T: ::serde::de::DeserializeOwned + ::serde::Serialize,
        {
            let typed = if string_wire {
                let instance = ::serde_json::Value::String(raw.to_string());
                validate(target, location, &instance)?;
                ::serde_json::from_value(instance).map_err(|_| generated_contract_error())?
            } else {
                ::serde_json::from_value(::serde_json::Value::String(raw.to_string()))
                    .or_else(|_| ::serde_json::from_str(raw))
                    .map_err(|_| malformed_parameter(location))?
            };
            validate_parameter(target, location, &typed)?;
            Ok(typed)
        }

        pub(crate) fn validate_string_parameter(
            target: &str,
            location: &str,
            raw: &str,
        ) -> ::std::result::Result<(), RequestValidationRejection> {
            validate(
                target,
                location,
                &::serde_json::Value::String(raw.to_string()),
            )
        }

        pub(crate) fn validate_parameter<T>(
            target: &str,
            location: &str,
            value: &T,
        ) -> ::std::result::Result<(), RequestValidationRejection>
        where
            T: ::serde::Serialize + ?Sized,
        {
            let instance = ::serde_json::to_value(value)
                .map_err(|_| generated_contract_error())?;
            validate(target, location, &instance)
        }

        pub(crate) fn parse_cookies(
            headers: &::axum::http::HeaderMap,
        ) -> ::std::result::Result<
            ::std::collections::BTreeMap<String, String>,
            RequestValidationRejection,
        > {
            let mut cookies = ::std::collections::BTreeMap::new();
            for header in headers.get_all(::axum::http::header::COOKIE).iter() {
                let line = header.to_str().map_err(|_| malformed_parameter("/cookie"))?;
                for field in line.split(';') {
                    let field = field.trim();
                    if field.is_empty() {
                        continue;
                    }
                    let (name, value) = field
                        .split_once('=')
                        .ok_or_else(|| malformed_parameter("/cookie"))?;
                    let name = name.trim();
                    if name.is_empty() || cookies.insert(name.to_string(), value.to_string()).is_some() {
                        return Err(malformed_parameter("/cookie"));
                    }
                }
            }
            Ok(cookies)
        }

        async fn read_body(
            body: ::axum::body::Body,
            max_body_bytes: usize,
        ) -> ::std::result::Result<::axum::body::Bytes, RequestValidationRejection> {
            ::axum::body::to_bytes(body, max_body_bytes)
                .await
                .map_err(|error| {
                    let source = ::std::error::Error::source(&error);
                    if source.is_some_and(|source| {
                        source.is::<::http_body_util::LengthLimitError>()
                    }) {
                        request_body_too_large()
                    } else {
                        malformed_request()
                    }
                })
        }

        fn media_type_is(content_type: &str, expected: &str) -> bool {
            let Ok(content_type) = content_type.parse::<::mime::Mime>() else {
                return false;
            };
            let Ok(expected) = expected.parse::<::mime::Mime>() else {
                return false;
            };
            content_type.type_() == expected.type_()
                && content_type.subtype() == expected.subtype()
        }

        pub(crate) fn validate(
            target: &str,
            location: &str,
            instance: &::serde_json::Value,
        ) -> ::std::result::Result<(), RequestValidationRejection> {
            let validators = VALIDATORS
                .as_ref()
                .map_err(|_| generated_contract_error())?;
            let Some(validator) = validators.get(target) else {
                return Err(generated_contract_error());
            };
            let mut errors: ::std::vec::Vec<InvalidParameter> = ::std::vec::Vec::new();
            for error in validator.iter_errors(instance) {
                let keyword = error.kind().keyword();
                let (code, message) = public_violation(keyword);
                let mut pointer = format!("{location}{}", error.instance_path());
                if let ::jsonschema::error::ValidationErrorKind::Required { property } = error.kind() {
                    if let Some(property) = property.as_str() {
                        pointer.push('/');
                        pointer.push_str(&escape_pointer_token(property));
                    }
                }
                let violation = InvalidParameter {
                    code: code.to_string(),
                    location: pointer,
                    message: message.to_string(),
                };
                if !errors.contains(&violation) {
                    errors.push(violation);
                    if errors.len() == MAX_VALIDATION_ERRORS {
                        break;
                    }
                }
            }
            errors.sort_by(|left, right| {
                (&left.location, &left.code, &left.message)
                    .cmp(&(&right.location, &right.code, &right.message))
            });
            if errors.is_empty() {
                return Ok(());
            }
            Err(RequestValidationRejection(ProblemDetails {
                r#type: "https://openapi-to-rust.dev/problems/validation".to_string(),
                title: "Request validation failed".to_string(),
                status: 422,
                code: "request_validation_failed".to_string(),
                errors,
            }))
        }

        pub(crate) fn malformed_request() -> RequestValidationRejection {
            public_problem(
                400,
                "https://openapi-to-rust.dev/problems/malformed-request",
                "Malformed request",
                "malformed_request",
            )
        }

        pub(crate) fn malformed_parameter(location: &str) -> RequestValidationRejection {
            parameter_problem(
                400,
                "https://openapi-to-rust.dev/problems/malformed-parameter",
                "Malformed request parameter",
                "malformed_parameter",
                "malformed",
                location,
                "is malformed",
            )
        }

        pub(crate) fn missing_parameter(location: &str) -> RequestValidationRejection {
            parameter_problem(
                422,
                "https://openapi-to-rust.dev/problems/validation",
                "Request validation failed",
                "request_validation_failed",
                "required",
                location,
                "is required",
            )
        }

        fn schema_parameter_problem(
            location: &str,
            code: &str,
            message: &str,
        ) -> RequestValidationRejection {
            parameter_problem(
                422,
                "https://openapi-to-rust.dev/problems/validation",
                "Request validation failed",
                "request_validation_failed",
                code,
                location,
                message,
            )
        }

        fn parameter_problem(
            status: u16,
            problem_type: &str,
            title: &str,
            code: &str,
            error_code: &str,
            location: &str,
            message: &str,
        ) -> RequestValidationRejection {
            RequestValidationRejection(ProblemDetails {
                r#type: problem_type.to_string(),
                title: title.to_string(),
                status,
                code: code.to_string(),
                errors: vec![InvalidParameter {
                    code: error_code.to_string(),
                    location: location.to_string(),
                    message: message.to_string(),
                }],
            })
        }

        pub(crate) fn request_body_too_large() -> RequestValidationRejection {
            public_problem(
                413,
                "https://openapi-to-rust.dev/problems/request-body-too-large",
                "Request body too large",
                "request_body_too_large",
            )
        }

        pub(crate) fn unsupported_media_type() -> RequestValidationRejection {
            public_problem(
                415,
                "https://openapi-to-rust.dev/problems/unsupported-media-type",
                "Unsupported media type",
                "unsupported_media_type",
            )
        }

        pub(crate) fn generated_contract_error() -> RequestValidationRejection {
            public_problem(
                500,
                "https://openapi-to-rust.dev/problems/generated-contract-error",
                "Internal server error",
                "generated_contract_error",
            )
        }

        fn public_problem(
            status: u16,
            problem_type: &str,
            title: &str,
            code: &str,
        ) -> RequestValidationRejection {
            RequestValidationRejection(ProblemDetails {
                r#type: problem_type.to_string(),
                title: title.to_string(),
                status,
                code: code.to_string(),
                errors: Vec::new(),
            })
        }

        fn public_violation(keyword: &str) -> (&'static str, &'static str) {
            match keyword {
                "required" => ("required", "is required"),
                "type" => ("type", "has an invalid type"),
                "enum" => ("enum", "has an unsupported value"),
                "const" => ("const", "has an unsupported value"),
                "format" => ("format", "has an invalid format"),
                "pattern" => ("pattern", "does not match the required format"),
                "minLength" => ("min_length", "does not meet the length constraint"),
                "maxLength" => ("max_length", "does not meet the length constraint"),
                "minimum" => ("minimum", "is outside the allowed range"),
                "maximum" => ("maximum", "is outside the allowed range"),
                "exclusiveMinimum" => ("exclusive_minimum", "is outside the allowed range"),
                "exclusiveMaximum" => ("exclusive_maximum", "is outside the allowed range"),
                "multipleOf" => ("multiple_of", "is outside the allowed range"),
                "minItems" => ("min_items", "does not meet the item-count constraint"),
                "maxItems" => ("max_items", "does not meet the item-count constraint"),
                "uniqueItems" => ("unique_items", "contains duplicate items"),
                "minProperties" => ("min_properties", "does not meet the property-count constraint"),
                "maxProperties" => ("max_properties", "does not meet the property-count constraint"),
                "additionalProperties" => ("additional_properties", "contains unsupported properties"),
                "unevaluatedProperties" => ("unevaluated_properties", "contains unsupported properties"),
                "anyOf" => ("any_of", "does not match the required shape"),
                "oneOf" => ("one_of", "does not match the required shape"),
                "not" => ("not", "does not match the required shape"),
                _ => ("invalid_value", "is invalid"),
            }
        }

        fn escape_pointer_token(value: &str) -> String {
            value.replace('~', "~0").replace('/', "~1")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::ParameterInfo;
    use std::collections::BTreeMap;

    #[test]
    fn openapi_30_nullable_is_a_real_union() {
        let normalized = normalize_schema(
            &json!({"type": "string", "enum": ["ok"], "nullable": true}),
            ValidationDraft::Draft4,
        );
        assert_eq!(normalized["anyOf"][1], json!({"type": "null"}));
        assert!(normalized["anyOf"][0].get("nullable").is_none());
    }

    #[test]
    fn local_refs_are_embedded_and_external_refs_are_rejected() {
        let context = ValidationContext {
            openapi_version: "3.1.0".to_string(),
            component_schemas: BTreeMap::from([(
                "Payload".to_string(),
                json!({"type": "string", "minLength": 3}),
            )]),
            ..Default::default()
        };
        let operation = OperationInfo {
            operation_id: "createPayload".to_string(),
            parameters: vec![ParameterInfo {
                name: "payload".to_string(),
                location: "query".to_string(),
                required: true,
                schema_ref: Some("Payload".to_string()),
                rust_type: "String".to_string(),
                description: None,
                enum_values: None,
                enum_varnames: None,
                rust_ident: None,
                query_serialization: None,
                validation_schema: Some(json!({"$ref": "#/components/schemas/Payload"})),
            }],
            ..Default::default()
        };
        let bundle = prepare_validation_bundle(&context, &[&operation]).unwrap();
        assert!(bundle.document_json.contains("minLength"));
        assert!(bundle.document_json.contains(BUNDLE_ID));

        let legacy_context = ValidationContext {
            openapi_version: "3.0.3".to_string(),
            ..context.clone()
        };
        let legacy_bundle = prepare_validation_bundle(&legacy_context, &[&operation]).unwrap();
        assert!(legacy_bundle.document_json.contains("definitions"));

        let mut external = operation.clone();
        external.parameters[0].validation_schema =
            Some(json!({"$ref": "https://example.invalid/schema.json"}));
        assert!(matches!(
            prepare_validation_bundle(&context, &[&external]),
            Err(ValidationPreparationError::UnsupportedReference { .. })
        ));

        let mut reserved = operation.clone();
        reserved.parameters[0].validation_schema =
            Some(json!({"$ref": format!("{BUNDLE_ID}#/properties/query") }));
        assert!(matches!(
            prepare_validation_bundle(&context, &[&reserved]),
            Err(ValidationPreparationError::UnsupportedReference { .. })
        ));
    }

    #[test]
    fn reference_like_data_is_not_rewritten() {
        let context = ValidationContext {
            openapi_version: "3.1.0".to_string(),
            component_schemas: BTreeMap::from([("Payload".to_string(), json!({"type": "string"}))]),
            ..Default::default()
        };
        let literal = json!({"$ref": "#/components/schemas/Payload"});
        let operation = OperationInfo {
            operation_id: "literalReference".to_string(),
            parameters: vec![ParameterInfo {
                name: "payload".to_string(),
                location: "query".to_string(),
                required: true,
                schema_ref: None,
                rust_type: "serde_json::Value".to_string(),
                description: None,
                enum_values: None,
                enum_varnames: None,
                rust_ident: None,
                query_serialization: None,
                validation_schema: Some(json!({"const": literal})),
            }],
            ..Default::default()
        };

        let bundle = prepare_validation_bundle(&context, &[&operation]).unwrap();
        let document: Value = serde_json::from_str(&bundle.document_json).unwrap();
        let target = bundle
            .target_for("literalReference", "query", Some("payload"))
            .unwrap();
        assert_eq!(
            document
                .pointer(target.pointer.trim_start_matches('#'))
                .unwrap()["const"],
            literal
        );
    }

    #[test]
    fn pointer_tokens_are_escaped() {
        assert_eq!(escape_pointer_token("a~/b"), "a~0~1b");
        assert_eq!(unescape_pointer_token("a~0~1b"), "a~/b");
    }

    #[test]
    fn constrained_inline_target_is_compiled_at_exported_pointer() {
        let context = ValidationContext {
            openapi_version: "3.1.0".to_string(),
            ..Default::default()
        };
        let operation = OperationInfo {
            operation_id: "cookieConstraint".to_string(),
            parameters: vec![ParameterInfo {
                name: "session".to_string(),
                location: "cookie".to_string(),
                required: true,
                schema_ref: None,
                rust_type: "String".to_string(),
                description: None,
                enum_values: None,
                enum_varnames: None,
                rust_ident: None,
                query_serialization: None,
                validation_schema: Some(json!({"type": "string", "maxLength": 4})),
            }],
            ..Default::default()
        };
        let bundle = prepare_validation_bundle(&context, &[&operation]).unwrap();
        let target = bundle
            .target_for("cookieConstraint", "cookie", Some("session"))
            .unwrap();
        let document: Value = serde_json::from_str(&bundle.document_json).unwrap();
        let validators = jsonschema::options()
            .with_draft(jsonschema::Draft::Draft202012)
            .build_map(&document)
            .unwrap();
        let validator = validators.get(&target.pointer).unwrap_or_else(|| {
            panic!(
                "target {} missing; validator keys: {:?}",
                target.pointer,
                validators.keys().collect::<Vec<_>>()
            )
        });
        assert!(!validator.is_valid(&json!("TOP_SECRET")));
    }
}
