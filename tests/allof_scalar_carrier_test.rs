use openapi_to_rust::analysis::{SchemaType, UntypedReason};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn scalar_allof_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "scalar allOf carriers", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "ScalarAlias": {
                    "allOf": [
                        { "type": "string" },
                        { "description": "alias annotation", "x-doc-source": "test" }
                    ]
                },
                "Envelope": {
                    "type": "object",
                    "required": [
                        "required_nullable",
                        "enum_value",
                        "flag",
                        "items",
                        "scalar_alias"
                    ],
                    "properties": {
                        "required_nullable": {
                            "allOf": [
                                {
                                    "type": "string",
                                    "format": "uuid",
                                    "nullable": true
                                },
                                {
                                    "nullable": false,
                                    "description": "neutral sibling"
                                }
                            ]
                        },
                        "enum_value": {
                            "allOf": [
                                { "type": "string", "enum": ["ready", "done"] },
                                { "description": "enum annotation" }
                            ]
                        },
                        "flag": {
                            "allOf": [
                                { "type": "boolean" },
                                { "default": true, "x-generated": true }
                            ]
                        },
                        "items": {
                            "allOf": [
                                {
                                    "type": "array",
                                    "items": { "type": "integer", "format": "int32" }
                                },
                                { "description": "array annotation" }
                            ]
                        },
                        "scalar_alias": { "$ref": "#/components/schemas/ScalarAlias" }
                    }
                },
                "UnsafeIntersection": {
                    "allOf": [
                        { "type": "string" },
                        { "maxLength": 5 }
                    ]
                },
                "Base": {
                    "type": "object",
                    "required": ["base"],
                    "properties": { "base": { "type": "string" } }
                },
                "Derived": {
                    "allOf": [
                        { "$ref": "#/components/schemas/Base" },
                        {
                            "type": "object",
                            "required": ["child"],
                            "properties": { "child": { "type": "integer" } }
                        }
                    ]
                }
            }
        }
    })
}

fn analyze() -> openapi_to_rust::SchemaAnalysis {
    SchemaAnalyzer::new(scalar_allof_spec())
        .expect("scalar allOf spec should parse")
        .analyze()
        .expect("scalar allOf spec should analyze")
}

#[test]
fn scalar_array_and_enum_allof_carriers_keep_their_types_and_nullability() {
    let analysis = analyze();
    let SchemaType::Object { properties, .. } = &analysis.schemas["Envelope"].schema_type else {
        panic!("Envelope should be an object");
    };
    assert!(properties["required_nullable"].nullable);
    assert!(matches!(
        properties["required_nullable"].schema_type,
        SchemaType::Primitive { ref rust_type, .. } if rust_type == "uuid::Uuid"
    ));
    assert!(matches!(
        properties["flag"].schema_type,
        SchemaType::Primitive { ref rust_type, .. } if rust_type == "bool"
    ));
    assert!(matches!(
        properties["items"].schema_type,
        SchemaType::Array { .. }
    ));
    assert!(matches!(
        analysis.schemas["ScalarAlias"].schema_type,
        SchemaType::Primitive { ref rust_type, .. } if rust_type == "String"
    ));

    assert!(matches!(
        analysis.schemas["UnsafeIntersection"].schema_type,
        SchemaType::Untyped {
            reason: UntypedReason::UnrepresentableComposition,
            ..
        }
    ));
    let SchemaType::Object {
        properties: derived,
        ..
    } = &analysis.schemas["Derived"].schema_type
    else {
        panic!("object inheritance must remain an object");
    };
    assert!(derived.contains_key("base"));
    assert!(derived.contains_key("child"));
}

#[test]
fn generated_scalar_allof_carriers_round_trip_exact_wire_values() {
    let mut analysis = analyze();
    let code = CodeGenerator::new(GeneratorConfig {
        module_name: "scalar_allof_carriers".into(),
        enable_async_client: false,
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("scalar allOf carriers should generate");
    assert!(
        code.contains("pub required_nullable: Option<uuid::Uuid>"),
        "nullable UUID carrier should not become a struct. Code:\n{code}"
    );
    assert!(
        code.contains("pub items: Vec<i32>"),
        "array carrier should retain its item type. Code:\n{code}"
    );
    assert!(
        code.contains("pub type UnsafeIntersection = serde_json::Value"),
        "unsafe intersections should stay opaque. Code:\n{code}"
    );

    let temp = tempfile::TempDir::new().expect("scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(temp.path().join("src/generated.rs"), code).expect("generated module");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "scalar-allof-carrier-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["serde"] }
"#,
    )
    .expect("scratch manifest");
    std::fs::write(
        temp.path().join("src/main.rs"),
        r##"#![allow(dead_code)]
mod generated;

fn check(input: serde_json::Value) {
    let hydrated: generated::Envelope = serde_json::from_value(input.clone()).unwrap();
    let output = serde_json::to_value(hydrated).unwrap();
    assert_eq!(output, input);
    let hydrated_again: generated::Envelope = serde_json::from_value(output.clone()).unwrap();
    assert_eq!(serde_json::to_value(hydrated_again).unwrap(), output);
}

fn main() {
    check(serde_json::json!({
        "required_nullable": null,
        "enum_value": "ready",
        "flag": true,
        "items": [1, 2],
        "scalar_alias": "alias"
    }));
    check(serde_json::json!({
        "required_nullable": "123e4567-e89b-12d3-a456-426614174000",
        "enum_value": "done",
        "flag": false,
        "items": [],
        "scalar_alias": "other"
    }));
}
"##,
    )
    .expect("scratch main");

    let output = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/scalar-allof-carrier-smoke"),
        )
        .output()
        .expect("cargo run");
    assert!(
        output.status.success(),
        "generated scalar allOf round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
