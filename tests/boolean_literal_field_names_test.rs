use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::fs;
use std::process::Command;

fn literal_key_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "Boolean literal fields", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "LiteralKeys": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "true": { "type": "boolean" },
                        "false": { "type": "boolean" },
                        "true_field": { "type": "string" },
                        "false_field": { "type": "string" },
                        "type": { "type": "string" }
                    },
                    "required": ["true", "false", "true_field", "false_field", "type"]
                }
            }
        }
    })
}

fn analyze_and_generate() -> (openapi_to_rust::analysis::SchemaAnalysis, String) {
    let mut analysis = SchemaAnalyzer::new(literal_key_spec())
        .expect("literal-key spec should parse")
        .analyze()
        .expect("literal-key spec should analyze");
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("literal-key model should generate legal Rust");
    (analysis, generated)
}

#[test]
fn analyzer_and_generator_emit_stable_boolean_literal_field_names() {
    let (analysis, generated) = analyze_and_generate();
    let SchemaType::Object {
        properties,
        required,
        ..
    } = &analysis.schemas["LiteralKeys"].schema_type
    else {
        panic!("LiteralKeys should analyze as an object");
    };
    for wire_name in ["true", "false", "true_field", "false_field", "type"] {
        assert!(properties.contains_key(wire_name));
        assert!(required.contains(wire_name));
    }

    let compact = generated.split_whitespace().collect::<String>();
    for expected in [
        r#"#[serde(rename="false")]pubfalse_field:bool"#,
        r#"#[serde(rename="false_field")]pubfalse_field_2:String"#,
        r#"#[serde(rename="true")]pubtrue_field:bool"#,
        r#"#[serde(rename="true_field")]pubtrue_field_2:String"#,
        "pubr#type:String",
    ] {
        assert!(
            compact.contains(expected),
            "missing generated fragment {expected:?}:\n{generated}"
        );
    }
    assert!(!compact.contains("pubtrue:"));
    assert!(!compact.contains("pubfalse:"));
    assert!(!compact.contains("pubr#true:"));
    assert!(!compact.contains("pubr#false:"));
}

#[test]
fn generated_boolean_literal_fields_compile_and_round_trip_exact_wire_keys() {
    let (_, generated) = analyze_and_generate();
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "boolean-literal-field-roundtrip-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    fs::create_dir(temp.path().join("src")).expect("create scratch source directory");

    let mut crate_source = generated;
    crate_source.push_str(
        r#"
#[cfg(test)]
mod tests {
    use super::LiteralKeys;

    #[test]
    fn required_boolean_literal_keys_round_trip() {
        let input = serde_json::json!({
            "true": true,
            "false": false,
            "true_field": "true collision",
            "false_field": "false collision",
            "type": "ordinary keyword"
        });
        let hydrated: LiteralKeys =
            serde_json::from_value(input.clone()).expect("hydrate literal keys");
        assert!(hydrated.true_field);
        assert!(!hydrated.false_field);
        assert_eq!(hydrated.true_field_2, "true collision");
        assert_eq!(hydrated.false_field_2, "false collision");
        assert_eq!(hydrated.r#type, "ordinary keyword");
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
        assert!(serde_json::from_value::<LiteralKeys>(serde_json::json!({
            "true": true,
            "true_field": "missing false",
            "false_field": "present",
            "type": "present"
        }))
        .is_err());
    }
}
"#,
    );
    fs::write(temp.path().join("src/lib.rs"), crate_source).expect("write scratch source");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/boolean-literal-field-roundtrip-smoke"),
        )
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .expect("run generated round-trip test");
    assert!(
        output.status.success(),
        "generated literal-key round-trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
