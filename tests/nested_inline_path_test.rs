use openapi_to_rust::analysis::{ObjectAdditionalProperties, SchemaType};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn nested_inline_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "nested inline paths", "version": "1" },
        "paths": {},
        "components": { "schemas": {
            "Root": {
                "type": "object",
                "additionalProperties": false,
                "required": ["created_by_user", "org", "wrapper", "users", "composed", "extras"],
                "properties": {
                    "created_by_user": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["data"],
                        "properties": {
                            "data": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["name"],
                                "properties": { "name": { "type": "string" } }
                            }
                        }
                    },
                    "org": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["data"],
                        "properties": {
                            "data": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["slug"],
                                "properties": { "slug": { "type": "string" } }
                            }
                        }
                    },
                    "wrapper": {
                        "type": "object",
                        "required": ["user"],
                        "properties": {
                            "user": {
                                "type": "object",
                                "required": ["email"],
                                "properties": { "email": { "type": "string" } }
                            }
                        }
                    },
                    "users": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["user"],
                            "properties": {
                                "user": {
                                    "type": "object",
                                    "required": ["active"],
                                    "properties": { "active": { "type": "boolean" } }
                                }
                            }
                        }
                    },
                    "composed": {
                        "allOf": [{
                            "type": "object",
                            "required": ["data"],
                            "properties": {
                                "data": {
                                    "type": "object",
                                    "required": ["token"],
                                    "properties": { "token": { "type": "string" } }
                                }
                            }
                        }]
                    },
                    "extras": {
                        "type": "object",
                        "additionalProperties": {
                            "type": "object",
                            "required": ["label"],
                            "properties": { "label": { "type": "string" } }
                        }
                    }
                }
            }
        } }
    })
}

fn referenced_property<'a>(
    analysis: &'a openapi_to_rust::SchemaAnalysis,
    owner: &str,
    field: &str,
) -> &'a str {
    let SchemaType::Object { properties, .. } = &analysis.schemas[owner].schema_type else {
        panic!("{owner} should be an object");
    };
    let SchemaType::Reference { target } = &properties[field].schema_type else {
        panic!("{owner}.{field} should reference a named inline schema");
    };
    target
}

#[test]
fn every_nested_inline_container_uses_its_complete_owner_path() {
    let analysis = SchemaAnalyzer::new(nested_inline_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");

    assert_eq!(
        referenced_property(&analysis, "Root", "created_by_user"),
        "RootCreatedByUser"
    );
    assert_eq!(
        referenced_property(&analysis, "RootCreatedByUser", "data"),
        "RootCreatedByUserData"
    );
    assert_eq!(referenced_property(&analysis, "Root", "org"), "RootOrg");
    assert_eq!(
        referenced_property(&analysis, "RootOrg", "data"),
        "RootOrgData"
    );
    assert_eq!(
        referenced_property(&analysis, "RootWrapper", "user"),
        "RootWrapperUser"
    );
    assert_eq!(
        referenced_property(&analysis, "RootUsersItem", "user"),
        "RootUsersItemUser"
    );
    assert_eq!(
        referenced_property(&analysis, "RootComposed", "data"),
        "RootComposedData"
    );

    let SchemaType::Object {
        additional_properties,
        ..
    } = &analysis.schemas["RootExtras"].schema_type
    else {
        panic!("RootExtras should be an object");
    };
    let ObjectAdditionalProperties::Typed { value_type } = additional_properties else {
        panic!("RootExtras should retain typed additional properties");
    };
    let SchemaType::Reference { target } = value_type.as_ref() else {
        panic!("additional-property objects should be named");
    };
    assert_eq!(target, "RootExtrasAdditionalProperty");
}

#[test]
fn generated_nested_inline_shapes_round_trip_without_cross_path_overwrites() {
    let mut analysis = SchemaAnalyzer::new(nested_inline_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("spec should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod nested_inline_path_runtime {
    use super::Root;

    #[test]
    fn every_nested_shape_survives_hydration_and_serialization() {
        let input = serde_json::json!({
            "created_by_user": {"data": {"name": "Ada"}},
            "org": {"data": {"slug": "compiler-team"}},
            "wrapper": {"user": {"email": "ada@example.test"}},
            "users": [{"user": {"active": true}}],
            "composed": {"data": {"token": "secret"}},
            "extras": {"primary": {"label": "one"}}
        });
        let hydrated: Root = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "nested-inline-path-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    fs::create_dir(temp.path().join("src")).expect("create scratch source");
    fs::write(temp.path().join("src/lib.rs"), generated).expect("write generated source");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/nested-inline-path-smoke"),
        )
        .output()
        .expect("run generated nested-inline test");
    assert!(
        output.status.success(),
        "generated nested-inline round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
