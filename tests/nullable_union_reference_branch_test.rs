use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn nullable_union_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "nullable union reference branches", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "A": {
                    "type": "object",
                    "required": ["a"],
                    "properties": { "a": { "type": "string" } }
                },
                "NullableB": {
                    "type": "object",
                    "nullable": true,
                    "required": ["b"],
                    "properties": { "b": { "type": "string" } }
                },
                "NullableBAlias": { "$ref": "#/components/schemas/NullableB" },
                "OneDirect": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/A" },
                        { "$ref": "#/components/schemas/NullableB" }
                    ]
                },
                "OneChained": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/A" },
                        { "$ref": "#/components/schemas/NullableBAlias" }
                    ]
                },
                "AnyDirect": {
                    "anyOf": [
                        { "$ref": "#/components/schemas/A" },
                        { "$ref": "#/components/schemas/NullableB" }
                    ]
                },
                "AnyChained": {
                    "anyOf": [
                        { "$ref": "#/components/schemas/A" },
                        { "$ref": "#/components/schemas/NullableBAlias" }
                    ]
                }
            }
        }
    })
}

#[test]
fn direct_and_chained_union_reference_branches_retain_nullability() {
    let analysis = SchemaAnalyzer::new(nullable_union_spec())
        .expect("nullable-union spec should parse")
        .analyze()
        .expect("nullable-union spec should analyze");

    for (union_name, nullable_target) in [
        ("OneDirect", "NullableB"),
        ("OneChained", "NullableBAlias"),
        ("AnyDirect", "NullableB"),
        ("AnyChained", "NullableBAlias"),
    ] {
        let SchemaType::Union { variants, .. } = &analysis.schemas[union_name].schema_type else {
            panic!("{union_name} should be an untagged union");
        };
        assert_eq!(variants.len(), 2, "{union_name}: {variants:?}");
        assert!(!variants[0].nullable, "A must remain non-nullable");
        assert_eq!(variants[1].target, nullable_target);
        assert!(
            variants[1].nullable,
            "{union_name} must retain target nullability: {variants:?}"
        );
    }

    let mut generated_analysis = analysis;
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut generated_analysis)
        .expect("nullable union references should generate");
    let compact = generated.split_whitespace().collect::<String>();
    assert!(compact.contains("NullableB(Option<NullableB>)"));
    assert!(compact.contains("NullableBAlias(Option<NullableBAlias>)"));
    assert!(
        !compact.contains("A(Option<A>)"),
        "non-null branches must not be widened:\n{generated}"
    );
}

#[test]
fn generated_nullable_union_branches_round_trip_null_and_objects() {
    let mut analysis = SchemaAnalyzer::new(nullable_union_spec())
        .expect("nullable-union spec should parse")
        .analyze()
        .expect("nullable-union spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("nullable union references should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod nullable_union_roundtrip {
    use super::{AnyChained, AnyDirect, OneChained, OneDirect};
    use serde::{Serialize, de::DeserializeOwned};

    fn stable<T>(input: serde_json::Value)
    where
        T: DeserializeOwned + Serialize,
    {
        let hydrated: T = serde_json::from_value(input.clone()).expect("hydrate");
        let output = serde_json::to_value(hydrated).expect("serialize");
        assert_eq!(output, input);
        let hydrated_again: T = serde_json::from_value(output).expect("rehydrate");
        assert_eq!(serde_json::to_value(hydrated_again).unwrap(), input);
    }

    #[test]
    fn every_union_accepts_its_source_valid_shapes() {
        for input in [
            serde_json::json!(null),
            serde_json::json!({"a": "first"}),
            serde_json::json!({"b": "second"}),
        ] {
            stable::<OneDirect>(input.clone());
            stable::<OneChained>(input.clone());
            stable::<AnyDirect>(input.clone());
            stable::<AnyChained>(input);
        }
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "nullable-union-reference-roundtrip-smoke"
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
                .join("target/nullable-union-reference-roundtrip-smoke"),
        )
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .expect("run generated round-trip test");
    assert!(
        output.status.success(),
        "generated nullable-union round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
