use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn null_enum_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "null-only enum", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "NullEnum": { "enum": [null] },
                "NullConst": { "const": null },
                "StringNull": { "type": "string", "enum": ["null"] },
                "NullableStatus": {
                    "type": "string", "enum": ["ready"], "nullable": true
                },
                "Payload": {
                    "type": "object",
                    "required": ["value"],
                    "properties": { "value": { "type": "string" } }
                },
                "NullOrPayload": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/NullEnum" },
                        { "$ref": "#/components/schemas/Payload" }
                    ]
                },
                "Envelope": {
                    "type": "object",
                    "required": ["enum_null", "const_null", "text", "status", "composed"],
                    "properties": {
                        "enum_null": { "$ref": "#/components/schemas/NullEnum" },
                        "const_null": { "$ref": "#/components/schemas/NullConst" },
                        "text": { "$ref": "#/components/schemas/StringNull" },
                        "status": { "$ref": "#/components/schemas/NullableStatus" },
                        "composed": { "$ref": "#/components/schemas/NullOrPayload" }
                    }
                }
            }
        }
    })
}

#[test]
fn analyzer_and_generator_keep_json_null_distinct_from_string_enums() {
    let analysis = SchemaAnalyzer::new(null_enum_spec())
        .expect("null-enum spec should parse")
        .analyze()
        .expect("null-enum spec should analyze");
    for name in ["NullEnum", "NullConst"] {
        assert!(matches!(
            &analysis.schemas[name].schema_type,
            SchemaType::Primitive { rust_type, .. } if rust_type == "()"
        ));
    }
    assert!(matches!(
        &analysis.schemas["StringNull"].schema_type,
        SchemaType::StringEnum { values } if values == &["null"]
    ));
    assert!(matches!(
        &analysis.schemas["NullableStatus"].schema_type,
        SchemaType::StringEnum { values } if values == &["ready"]
    ));
    assert!(analysis.schemas["NullableStatus"].nullable);

    let mut generated_analysis = analysis;
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut generated_analysis)
        .expect("null-only schemas should generate");
    let compact = generated.split_whitespace().collect::<String>();
    assert!(compact.contains("pubtypeNullEnum=();"));
    assert!(compact.contains("pubtypeNullConst=();"));
    assert!(compact.contains("pubenumStringNull"));
    assert!(compact.contains("NullEnum(NullEnum)"));
    assert!(!compact.contains("pubenumNullEnum"));
}

#[test]
fn generated_null_only_types_round_trip_directly_and_in_composition() {
    let mut analysis = SchemaAnalyzer::new(null_enum_spec())
        .expect("null-enum spec should parse")
        .analyze()
        .expect("null-enum spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("null-only schemas should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod null_only_roundtrip {
    use super::{Envelope, NullEnum, NullOrPayload};

    #[test]
    fn null_is_exact_and_non_null_is_rejected() {
        let direct: NullEnum = serde_json::from_value(serde_json::json!(null)).unwrap();
        assert_eq!(serde_json::to_value(direct).unwrap(), serde_json::json!(null));
        assert!(serde_json::from_value::<NullEnum>(serde_json::json!("null")).is_err());

        for input in [serde_json::json!(null), serde_json::json!({"value": "known"})] {
            let value: NullOrPayload = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(value).unwrap(), input);
        }
    }

    #[test]
    fn required_null_fields_and_composed_null_are_stable() {
        let input = serde_json::json!({
            "enum_null": null,
            "const_null": null,
            "text": "null",
            "status": null,
            "composed": null
        });
        let hydrated: Envelope = serde_json::from_value(input.clone()).unwrap();
        let output = serde_json::to_value(hydrated).unwrap();
        assert_eq!(output, input);
        let stable: Envelope = serde_json::from_value(output).unwrap();
        assert_eq!(serde_json::to_value(stable).unwrap(), input);
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "null-only-enum-roundtrip-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    fs::create_dir(temp.path().join("src")).expect("create scratch source directory");
    fs::write(temp.path().join("src/lib.rs"), generated).expect("write generated source");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/null-only-enum-roundtrip-smoke"),
        )
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .expect("run generated round-trip test");
    assert!(
        output.status.success(),
        "generated null-only round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
