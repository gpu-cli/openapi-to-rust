use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn nullable_union_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "required nullable unions", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "Envelope": {
                    "type": "object",
                    "required": ["any_type", "one_type", "any_const", "one_enum"],
                    "properties": {
                        "any_type": {
                            "anyOf": [
                                { "type": "string" },
                                { "type": "integer" },
                                { "type": "null" }
                            ]
                        },
                        "one_type": {
                            "oneOf": [
                                { "type": "integer" },
                                { "type": "string" },
                                { "type": "null" }
                            ]
                        },
                        "any_const": {
                            "anyOf": [
                                { "type": "boolean" },
                                { "type": "array", "items": { "type": "string" } },
                                { "const": null }
                            ]
                        },
                        "one_enum": {
                            "oneOf": [
                                { "type": "number" },
                                { "type": "string" },
                                { "enum": [null] }
                            ]
                        }
                    }
                }
            }
        }
    })
}

fn generate() -> String {
    let mut analysis = SchemaAnalyzer::new(nullable_union_spec())
        .expect("nullable union spec should parse")
        .analyze()
        .expect("nullable union spec should analyze");
    CodeGenerator::new(GeneratorConfig {
        module_name: "required_nullable_unions".into(),
        enable_async_client: false,
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("nullable union types should generate")
}

#[test]
fn required_multi_branch_unions_retain_explicit_nullability() {
    let analysis = SchemaAnalyzer::new(nullable_union_spec())
        .expect("nullable union spec should parse")
        .analyze()
        .expect("nullable union spec should analyze");
    let envelope = &analysis.schemas["Envelope"].schema_type;
    let openapi_to_rust::analysis::SchemaType::Object { properties, .. } = envelope else {
        panic!("Envelope should analyze as an object: {envelope:?}");
    };
    for field in ["any_type", "one_type", "any_const", "one_enum"] {
        assert!(properties[field].nullable, "{field} must remain nullable");
    }

    let code = generate();
    for (field, type_name) in [
        ("any_type", "EnvelopeAnyType"),
        ("one_type", "EnvelopeOneType"),
        ("any_const", "EnvelopeAnyConst"),
        ("one_enum", "EnvelopeOneEnum"),
    ] {
        assert!(
            code.contains(&format!("pub {field}: Option<{type_name}>")),
            "required nullable {field} must use Option without changing its non-null union. Code:\n{code}"
        );
    }
}

#[test]
fn generated_required_multi_branch_unions_round_trip_null_and_non_null_values() {
    let code = generate();
    let temp = tempfile::TempDir::new().expect("scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(temp.path().join("src/generated.rs"), code).expect("generated module");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "required-multi-branch-nullable-union-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
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
        "any_type": null,
        "one_type": null,
        "any_const": null,
        "one_enum": null
    }));
    check(serde_json::json!({
        "any_type": "text",
        "one_type": 42,
        "any_const": true,
        "one_enum": "other"
    }));
    check(serde_json::json!({
        "any_type": 7,
        "one_type": "text",
        "any_const": ["a", "b"],
        "one_enum": 1.5
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
                .join("target/required-multi-branch-nullable-union-smoke"),
        )
        .output()
        .expect("cargo run");
    assert!(
        output.status.success(),
        "generated nullable union round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
