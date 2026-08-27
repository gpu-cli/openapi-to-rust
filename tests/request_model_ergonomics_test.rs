use openapi_to_rust::config::ClientSection;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn ergonomics_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "request model ergonomics", "version": "1.0.0" },
        "paths": {
            "/users/{user_id}": { "patch": {
                "operationId": "updateUser",
                "parameters": [{
                    "name": "user_id",
                    "in": "path",
                    "required": true,
                    "schema": { "type": "string" }
                }],
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/UpdateUserRequest"
                    } } }
                },
                "responses": { "200": { "description": "updated" } }
            }},
            "/invitations": { "post": {
                "operationId": "createInvitation",
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/CreateInvitationRequest"
                    } } }
                },
                "responses": { "201": { "description": "created" } }
            }},
            "/ignored": { "post": {
                "operationId": "ignoredOperation",
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/IgnoredRequest"
                    } } }
                },
                "responses": { "204": { "description": "ignored" } }
            }},
            "/aliased": { "post": {
                "operationId": "createAliased",
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/AliasRequest"
                    } } }
                },
                "responses": { "204": { "description": "created" } }
            }}
        },
        "components": { "schemas": {
            "UpdateUserRequest": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "first_name": { "type": ["string", "null"] },
                    "last_name": { "type": ["string", "null"] },
                    "primary_email_address_id": { "type": ["string", "null"] },
                    "profile_image": { "type": ["string", "null"] },
                    "skip_password_checks": { "type": "boolean" }
                }
            },
            "CreateInvitationRequest": {
                "type": "object",
                "required": ["email_address", "external_id"],
                "additionalProperties": { "type": "string" },
                "properties": {
                    "email_address": { "type": "string" },
                    "external_id": { "type": "string", "nullable": true },
                    "first_name": { "type": "string" },
                    "nullable_note": { "type": ["string", "null"] },
                    "nullable_note_null": { "type": "string" },
                    "nullable_note_absent": { "type": "string" },
                    "notify": { "type": "boolean" },
                    "new": { "type": "string" },
                    "with_new": { "type": "string" },
                    "build": { "type": "integer" },
                    "connectionString": { "type": "string" },
                    "connection_string": { "type": "string" },
                    "additional_properties": { "type": "string" }
                }
            },
            "IgnoredRequest": {
                "type": "object",
                "required": ["required_value"],
                "properties": {
                    "required_value": { "type": "string" },
                    "optional_value": { "type": "string" }
                }
            },
            "AllOptionalResponse": {
                "type": "object",
                "additionalProperties": false,
                "properties": { "cursor": { "type": "string" } }
            },
            "NonRequestMixed": {
                "type": "object",
                "required": ["id"],
                "additionalProperties": false,
                "properties": {
                    "id": { "type": "string" },
                    "label": { "type": "string" }
                }
            },
            "AliasRequest": {
                "$ref": "#/components/schemas/ActualAliasRequest"
            },
            "ActualAliasRequest": {
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": {
                    "id": { "type": "string" },
                    "note": { "type": "string" }
                }
            }
        } }
    })
}

fn generated_types_for(
    spec: serde_json::Value,
    config: GeneratorConfig,
) -> (String, openapi_to_rust::SchemaAnalysis) {
    let mapper = openapi_to_rust::TypeMapper::new(config.types.clone());
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, mapper).expect("valid ergonomics spec");
    let mut analysis = analyzer.analyze().expect("analyze ergonomics spec");
    let result = CodeGenerator::new(config)
        .generate_all(&mut analysis)
        .expect("generate request models");
    let types = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("types.rs"))
        .expect("types.rs")
        .content
        .clone();
    (types, analysis)
}

fn generated_types(config: GeneratorConfig) -> (String, openapi_to_rust::SchemaAnalysis) {
    generated_types_for(ergonomics_spec(), config)
}

fn struct_derives(types: &str, name: &str, derive: &str) -> bool {
    let file = syn::parse_file(types).expect("generated types parse as Rust");
    file.items.iter().any(|item| {
        let syn::Item::Struct(item) = item else {
            return false;
        };
        item.ident == name
            && item.attrs.iter().any(|attribute| {
                attribute.path().is_ident("derive")
                    && attribute.meta.require_list().is_ok_and(|list| {
                        list.tokens.to_string().split(',').any(|value| {
                            value.trim().split_whitespace().collect::<String>() == derive
                        })
                    })
            })
    })
}

#[test]
fn all_optional_objects_derive_default_without_inventing_required_data() {
    let (types, _) = generated_types(GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });

    // Clerk-style PATCH body from GitHub issue #6.
    assert!(struct_derives(&types, "UpdateUserRequest", "Default"));
    // Default is a general object-model capability, not request-only.
    assert!(struct_derives(&types, "AllOptionalResponse", "Default"));
    // A required field must never be fabricated through Rust's type default.
    assert!(!struct_derives(
        &types,
        "CreateInvitationRequest",
        "Default"
    ));
    assert!(!struct_derives(&types, "NonRequestMixed", "Default"));
}

#[test]
fn mixed_request_root_has_required_constructor_and_every_optional_setter() {
    let (types, _) = generated_types(GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let compact = types.split_whitespace().collect::<String>();

    assert!(compact.contains("pubstructCreateInvitationRequestBuilder"));
    assert!(compact.contains(
        "pubfnbuilder(email_address:String,external_id:Option<String>,)->CreateInvitationRequestBuilder"
    ));
    assert!(compact.contains("pubfnfirst_name(mutself,first_name:String)->Self"));
    assert!(compact.contains("pubfnnullable_note(mutself,nullable_note:String)->Self"));
    assert!(compact.contains("pubfnnullable_note_null(mutself)->Self"));
    assert!(compact.contains("pubfnnullable_note_absent(mutself)->Self"));
    assert!(compact.contains("pubfnnullable_note_null_2(mutself,nullable_note_null:String)->Self"));
    assert!(
        compact.contains("pubfnnullable_note_absent_2(mutself,nullable_note_absent:String)->Self")
    );
    assert!(compact.contains("pubfnnotify(mutself,notify:bool)->Self"));
    assert!(compact.contains("pubfnwith_new(mutself,new:String)->Self"));
    assert!(compact.contains("pubfnwith_new_2(mutself,with_new:String)->Self"));
    assert!(compact.contains("pubfnwith_build(mutself,build:i64)->Self"));
    assert!(
        compact
            .contains("pubfnadditional_properties_2(mutself,additional_properties_2:String)->Self")
    );
    assert!(compact.contains("pubfnconnection_string(mutself,connection_string:String)->Self"));
    assert!(compact.contains("pubfnconnection_string_2(mutself,connection_string_2:String)->Self"));
    assert!(compact.contains("pubfnadditional_properties("));
    assert!(compact.contains("pubfnbuild(self)->CreateInvitationRequest"));

    // Builders are intentionally limited to actual request-body roots.
    assert!(!types.contains("NonRequestMixedBuilder"));
    // All-optional bodies use `Default` instead of a redundant zero-arg builder.
    assert!(!types.contains("UpdateUserRequestBuilder"));
}

#[test]
fn request_aliases_expose_the_target_objects_builder() {
    let (types, _) = generated_types(GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });

    assert!(types.contains("pub type AliasRequest = ActualAliasRequest"));
    assert!(types.contains("pub struct ActualAliasRequestBuilder"));
}

#[test]
fn option_wrapped_codec_fields_use_the_option_codec() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "codec wrapping", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": { "CodecHolder": {
            "type": "object",
            "additionalProperties": false,
            "required": ["date_with_default", "overridden_bytes"],
            "properties": {
                "date_with_default": {
                    "type": "string",
                    "format": "date",
                    "default": "2026-01-01"
                },
                "overridden_bytes": { "type": "string", "format": "byte" }
            }
        } } }
    });
    let mut config = GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    };
    config.types.date = openapi_to_rust::type_mapping::DateStrategy::Time;
    config
        .nullable_field_overrides
        .insert("CodecHolder.overridden_bytes".into(), true);
    let (types, _) = generated_types_for(spec, config);
    let compact = types.split_whitespace().collect::<String>();

    assert!(compact.contains("pubdate_with_default:Option<time::Date>"));
    assert!(compact.contains("with=\"time_date_format::option\""));
    assert!(compact.contains("puboverridden_bytes:Option<Vec<u8>>"));
    assert!(compact.contains("with=\"base64_serde::option\""));
}

#[test]
fn selective_client_pruning_keeps_selected_builder_root() {
    let (types, analysis) = generated_types(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec!["createInvitation".into()],
            prune_models: true,
        }),
        ..Default::default()
    });

    assert!(analysis.schemas.contains_key("CreateInvitationRequest"));
    assert!(!analysis.schemas.contains_key("IgnoredRequest"));
    assert!(types.contains("CreateInvitationRequestBuilder"));
    assert!(!types.contains("IgnoredRequestBuilder"));
}

#[test]
fn generated_request_defaults_and_builders_compile_without_new_dependencies() {
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    let output_dir = temp.path().join("src/generated");
    let mut analyzer = SchemaAnalyzer::new(ergonomics_spec()).expect("valid ergonomics spec");
    let mut analysis = analyzer.analyze().expect("analyze ergonomics spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "request_models".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator
        .generate_all(&mut analysis)
        .expect("generate request models");
    generator
        .write_files(&result)
        .expect("write generated files");

    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "request-model-ergonomics-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
use super::generated;

#[test]
fn exercise_generated_api() {
    let update = generated::UpdateUserRequest {
        first_name: Some(Some("Ada".to_string())),
        ..Default::default()
    };
    let empty_patch = generated::UpdateUserRequest::default();
    assert!(empty_patch.first_name.is_none());
    assert!(empty_patch.last_name.is_none());
    assert_eq!(serde_json::to_value(empty_patch).unwrap(), serde_json::json!({}));

    let mut extras = std::collections::BTreeMap::new();
    extras.insert("source".to_string(), "docs".to_string());
    let request = generated::CreateInvitationRequest::builder(
        "ada@example.com".to_string(),
        None,
    )
    .first_name("Ada".to_string())
    .nullable_note("transient".to_string())
    .nullable_note_null()
    .nullable_note_null_2("literal null suffix".to_string())
    .nullable_note_absent_2("literal absent suffix".to_string())
    .notify(true)
    .with_new("new value".to_string())
    .with_new_2("also new".to_string())
    .with_build(7)
    .connection_string("camel".to_string())
    .connection_string_2("snake".to_string())
    .additional_properties_2("declared".to_string())
    .additional_properties(extras)
    .build();
    assert_eq!(request.email_address, "ada@example.com");
    assert_eq!(request.first_name.as_deref(), Some("Ada"));
    assert_eq!(request.nullable_note, Some(None));
    assert_eq!(request.additional_properties["source"], "docs");
    let request_json = serde_json::to_value(&request).unwrap();
    assert_eq!(request_json["connectionString"], "camel");
    assert_eq!(request_json["nullable_note"], serde_json::Value::Null);
    assert_eq!(request_json["nullable_note_null"], "literal null suffix");
    assert_eq!(request_json["nullable_note_absent"], "literal absent suffix");
    assert_eq!(request_json["connection_string"], "snake");
    assert_eq!(request_json["additional_properties"], "declared");
    assert_eq!(request_json["source"], "docs");

    let direct = generated::CreateInvitationRequest::new(
        "grace@example.com".to_string(),
        Some("external-1".to_string()),
    );
    assert!(direct.first_name.is_none());
    assert!(direct.nullable_note.is_none());
    assert!(direct.additional_properties.is_empty());
    let aliased = generated::AliasRequest::builder("alias-id".to_string())
        .note("available through the alias".to_string())
        .build();
    assert_eq!(aliased.note.as_deref(), Some("available through the alias"));
    let absent_again = generated::CreateInvitationRequest::builder(
        "missing@example.com".to_string(),
        None,
    )
    .nullable_note_null()
    .nullable_note_absent()
    .build();
    assert!(absent_again.nullable_note.is_none());
    assert!(serde_json::to_value(absent_again)
        .unwrap()
        .get("nullable_note")
        .is_none());
    let _json = serde_json::to_value((update, request)).unwrap();
}
}
"#,
    )
    .expect("write scratch source");

    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/request-model-ergonomics-smoke"),
        )
        .output()
        .expect("run cargo check for generated request models");
    assert!(
        output.status.success(),
        "generated request models failed to compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
