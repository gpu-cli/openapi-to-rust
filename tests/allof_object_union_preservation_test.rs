use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn object_union_allof_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "object union allOf", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "RuleFeature": {
                    "allOf": [
                        {
                            "type": "object",
                            "required": ["name"],
                            "properties": { "name": { "type": "string" } }
                        },
                        {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "required": ["type"],
                                    "properties": {
                                        "type": { "type": "string", "const": "CARD" }
                                    }
                                },
                                {
                                    "type": "object",
                                    "required": ["type", "scope"],
                                    "properties": {
                                        "type": { "type": "string", "const": "VELOCITY" },
                                        "scope": { "type": "string" }
                                    }
                                }
                            ],
                            "discriminator": { "propertyName": "type" }
                        }
                    ]
                },
                "StringFilter": {
                    "type": "object",
                    "required": ["type", "op", "value"],
                    "properties": {
                        "type": { "type": "string", "enum": ["string"] },
                        "op": { "type": "string" },
                        "value": { "type": "string" }
                    }
                },
                "NumberFilter": {
                    "type": "object",
                    "required": ["type", "op", "value"],
                    "properties": {
                        "type": { "type": "string", "enum": ["number"] },
                        "op": { "type": "string" },
                        "value": { "type": "number" }
                    }
                },
                "ValueFilter": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/StringFilter" },
                        { "$ref": "#/components/schemas/NumberFilter" }
                    ]
                },
                "CustomFieldFilter": {
                    "allOf": [
                        { "$ref": "#/components/schemas/ValueFilter" },
                        {
                            "type": "object",
                            "required": ["key"],
                            "properties": { "key": { "type": "string" } }
                        }
                    ]
                },
                "SignerBase": {
                    "type": "object",
                    "properties": {
                        "email": { "type": "string", "nullable": true },
                        "language": { "type": "string" }
                    }
                },
                "Signer": {
                    "type": "object",
                    "required": ["email"],
                    "allOf": [
                        { "$ref": "#/components/schemas/SignerBase" },
                        {
                            "type": "object",
                            "properties": { "viewed": { "type": "boolean" } }
                        }
                    ]
                },
                "RootSibling": {
                    "type": "object",
                    "required": ["root_value"],
                    "properties": { "root_value": { "type": "string" } },
                    "allOf": [
                        {
                            "type": "object",
                            "required": ["child_value"],
                            "properties": { "child_value": { "type": "integer" } }
                        }
                    ]
                },
                "GitpodOutputSpec": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "key": { "type": "string" },
                        "command": { "type": "string" },
                        "prompt": { "type": "string" },
                        "boolean": { "type": "object", "properties": {} },
                        "string": { "type": "object", "properties": {} }
                    },
                    "allOf": [
                        {
                            "anyOf": [
                                { "required": ["command"] },
                                { "required": ["prompt"] },
                                {
                                    "not": {
                                        "anyOf": [
                                            { "required": ["command"] },
                                            { "required": ["prompt"] }
                                        ]
                                    }
                                }
                            ]
                        },
                        {
                            "anyOf": [
                                { "required": ["boolean"] },
                                { "required": ["string"] },
                                {
                                    "not": {
                                        "anyOf": [
                                            { "required": ["boolean"] },
                                            { "required": ["string"] }
                                        ]
                                    }
                                }
                            ]
                        }
                    ]
                }
            }
        }
    })
}

fn analyze() -> openapi_to_rust::SchemaAnalysis {
    SchemaAnalyzer::new(object_union_allof_spec())
        .expect("object union allOf spec should parse")
        .analyze()
        .expect("object union allOf spec should analyze")
}

#[test]
fn allof_objects_retain_inline_referenced_and_root_sibling_shapes() {
    let analysis = analyze();
    for name in ["RuleFeature", "CustomFieldFilter"] {
        let SchemaType::Object {
            properties,
            variant,
            ..
        } = &analysis.schemas[name].schema_type
        else {
            panic!("{name} should be an object with a flattened union");
        };
        assert!(variant.is_some(), "{name} must retain its union member");
        assert!(
            properties.contains_key(if name == "RuleFeature" { "name" } else { "key" }),
            "{name} must retain its sibling property"
        );
    }

    let SchemaType::Object {
        properties,
        required,
        ..
    } = &analysis.schemas["Signer"].schema_type
    else {
        panic!("Signer should be a merged object");
    };
    assert!(properties["email"].nullable);
    assert!(required.contains("email"));

    let SchemaType::Object {
        properties,
        required,
        ..
    } = &analysis.schemas["RootSibling"].schema_type
    else {
        panic!("RootSibling should be a merged object");
    };
    assert!(properties.contains_key("root_value"));
    assert!(properties.contains_key("child_value"));
    assert!(required.contains("root_value"));
    assert!(required.contains("child_value"));

    let SchemaType::Object {
        properties: output_spec,
        ..
    } = &analysis.schemas["GitpodOutputSpec"].schema_type
    else {
        panic!("GitpodOutputSpec should retain its root object shape");
    };
    for field in ["key", "command", "prompt", "boolean", "string"] {
        assert!(
            output_spec.contains_key(field),
            "Gitpod root sibling field {field} was dropped"
        );
    }
}

#[test]
fn generated_allof_objects_round_trip_union_fields_and_required_nulls_exactly() {
    let mut analysis = analyze();
    let code = CodeGenerator::new(GeneratorConfig {
        module_name: "object_union_allof".into(),
        enable_async_client: false,
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("object union allOf types should generate");
    assert!(
        code.contains("#[serde(flatten)]"),
        "allOf unions need a flattened carrier. Code:\n{code}"
    );
    let signer_start = code.find("pub struct Signer {").expect("Signer struct");
    let signer_end = code[signer_start..]
        .find("\n}")
        .map(|offset| signer_start + offset)
        .expect("Signer end");
    let signer = &code[signer_start..signer_end];
    assert!(
        signer.starts_with("pub struct Signer {\n    pub email: Option<String>,"),
        "required nullable email must serialize explicit null: {signer}"
    );

    let temp = tempfile::TempDir::new().expect("scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(temp.path().join("src/generated.rs"), code).expect("generated module");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "object-union-allof-smoke"
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

fn round_trip<T>(input: serde_json::Value)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let hydrated: T = serde_json::from_value(input.clone()).unwrap();
    let output = serde_json::to_value(hydrated).unwrap();
    assert_eq!(output, input);
    let hydrated_again: T = serde_json::from_value(output.clone()).unwrap();
    assert_eq!(serde_json::to_value(hydrated_again).unwrap(), output);
}

fn main() {
    round_trip::<generated::RuleFeature>(serde_json::json!({
        "name": "card",
        "type": "CARD"
    }));
    round_trip::<generated::RuleFeature>(serde_json::json!({
        "name": "velocity",
        "type": "VELOCITY",
        "scope": "card"
    }));
    round_trip::<generated::CustomFieldFilter>(serde_json::json!({
        "key": "status",
        "type": "string",
        "op": "eq",
        "value": "ready"
    }));
    round_trip::<generated::CustomFieldFilter>(serde_json::json!({
        "key": "amount",
        "type": "number",
        "op": "gt",
        "value": 10.5
    }));
    round_trip::<generated::Signer>(serde_json::json!({
        "email": null,
        "language": "en",
        "viewed": true
    }));
    round_trip::<generated::RootSibling>(serde_json::json!({
        "root_value": "root",
        "child_value": 7
    }));
    round_trip::<generated::GitpodOutputSpec>(serde_json::json!({
        "key": "coverage",
        "command": "collect-coverage",
        "boolean": {}
    }));
    round_trip::<generated::GitpodOutputSpec>(serde_json::json!({}));
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
                .join("target/object-union-allof-smoke"),
        )
        .output()
        .expect("cargo run");
    assert!(
        output.status.success(),
        "generated object/union allOf round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn multiple_union_intersections_are_reported_instead_of_silently_dropped() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "multiple unions", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "ImpossibleToProject": {
                    "allOf": [
                        { "oneOf": [{ "type": "string" }, { "type": "integer" }] },
                        { "anyOf": [{ "type": "boolean" }, { "type": "array" }] },
                        { "type": "object", "properties": { "id": { "type": "string" } } }
                    ]
                }
            }
        }
    });
    let error = SchemaAnalyzer::new(spec)
        .expect("multiple union spec should parse")
        .analyze()
        .expect_err("multiple allOf unions need an explicit diagnostic");
    assert!(
        error
            .to_string()
            .contains("intersects multiple union members"),
        "unexpected error: {error}"
    );
}
