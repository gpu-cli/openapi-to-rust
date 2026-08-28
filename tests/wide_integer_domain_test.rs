use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn wide_integer_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "wide integer domains", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "InRangeInt64": {
                    "type": "integer", "format": "int64",
                    "minimum": -100, "maximum": 100
                },
                "WideInt64": {
                    "type": "integer", "format": "int64",
                    "minimum": 0, "maximum": 1.8446744073709551e19
                },
                "WideDefault": {
                    "type": "integer",
                    "minimum": 0, "maximum": 9223372036854776000_u64
                },
                "ExampleOnly": {
                    "type": "integer",
                    "example": 18446744073709551615_u64
                },
                "NarrowInt32": {
                    "type": "integer", "format": "int32",
                    "minimum": -100, "maximum": 100
                },
                "WidenedInt32": {
                    "type": "integer", "format": "int32",
                    "minimum": 0, "maximum": 5000000000_u64
                },
                "Unsigned64": {
                    "type": "integer", "format": "uint64",
                    "minimum": 0, "maximum": 1.8446744073709551e19
                },
                "WideArray": {
                    "type": "array",
                    "items": {
                        "type": "integer", "format": "int64",
                        "minimum": 0, "maximum": 1.8446744073709551e19
                    }
                },
                "WideOrText": {
                    "anyOf": [
                        {
                            "type": "integer", "format": "int64",
                            "minimum": 0, "maximum": 1.8446744073709551e19
                        },
                        { "type": "string" }
                    ]
                },
                "Envelope": {
                    "type": "object",
                    "required": [
                        "in_range", "wide", "default_wide", "example_wide",
                        "narrow", "widened_32", "unsigned", "array", "union"
                    ],
                    "properties": {
                        "in_range": { "$ref": "#/components/schemas/InRangeInt64" },
                        "wide": { "$ref": "#/components/schemas/WideInt64" },
                        "default_wide": { "$ref": "#/components/schemas/WideDefault" },
                        "example_wide": { "$ref": "#/components/schemas/ExampleOnly" },
                        "narrow": { "$ref": "#/components/schemas/NarrowInt32" },
                        "widened_32": { "$ref": "#/components/schemas/WidenedInt32" },
                        "unsigned": { "$ref": "#/components/schemas/Unsigned64" },
                        "array": { "$ref": "#/components/schemas/WideArray" },
                        "union": { "$ref": "#/components/schemas/WideOrText" }
                    }
                }
            }
        }
    })
}

fn primitive_rust_type<'a>(
    analysis: &'a openapi_to_rust::analysis::SchemaAnalysis,
    name: &str,
) -> &'a str {
    let SchemaType::Primitive { rust_type, .. } = &analysis.schemas[name].schema_type else {
        panic!(
            "{name} should be primitive: {:?}",
            analysis.schemas[name].schema_type
        );
    };
    rust_type
}

#[test]
fn integer_width_follows_effective_schema_domain() {
    let analysis = SchemaAnalyzer::new(wide_integer_spec())
        .expect("wide-integer spec should parse")
        .analyze()
        .expect("wide-integer spec should analyze");
    for (name, expected) in [
        ("InRangeInt64", "i64"),
        ("WideInt64", "u64"),
        ("WideDefault", "u64"),
        ("ExampleOnly", "i128"),
        ("NarrowInt32", "i32"),
        ("WidenedInt32", "i64"),
        ("Unsigned64", "u64"),
    ] {
        assert_eq!(primitive_rust_type(&analysis, name), expected, "{name}");
    }

    let SchemaType::Array { item_type } = &analysis.schemas["WideArray"].schema_type else {
        panic!("WideArray should be an array");
    };
    assert!(matches!(
        item_type.as_ref(),
        SchemaType::Primitive { rust_type, .. } if rust_type == "u64"
    ));

    let SchemaType::Union { variants, .. } = &analysis.schemas["WideOrText"].schema_type else {
        panic!("WideOrText should be a union");
    };
    assert_eq!(variants[0].target, "u64");
    assert_eq!(variants[1].target, "String");

    let mut generated_analysis = analysis;
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut generated_analysis)
        .expect("wide integer domains should generate");
    let compact = generated.split_whitespace().collect::<String>();
    assert!(compact.contains("pubtypeWideInt64=u64;"));
    assert!(compact.contains("pubtypeWideArray=Vec<u64>;"));
    assert!(compact.contains("UnsignedInteger(u64)"));
    assert!(compact.contains("pubtypeInRangeInt64=i64;"));
    assert!(compact.contains("pubtypeNarrowInt32=i32;"));
    assert!(compact.contains("pubtypeUnsigned64=u64;"));
}

#[test]
fn generated_wide_integers_round_trip_exact_json_numbers() {
    let mut analysis = SchemaAnalyzer::new(wide_integer_spec())
        .expect("wide-integer spec should parse")
        .analyze()
        .expect("wide-integer spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("wide integer domains should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod wide_integer_roundtrip {
    use super::Envelope;

    #[test]
    fn values_above_i64_are_stable() {
        let input = serde_json::json!({
            "in_range": -100,
            "wide": 18446744073709551615_u64,
            "default_wide": 9223372036854775808_u64,
            "example_wide": 18446744073709551615_u64,
            "narrow": 100,
            "widened_32": 5000000000_u64,
            "unsigned": 18446744073709551615_u64,
            "array": [9223372036854775808_u64, 18446744073709551615_u64],
            "union": 18446744073709551615_u64
        });
        let hydrated: Envelope = serde_json::from_value(input.clone()).expect("hydrate");
        let output = serde_json::to_value(hydrated).expect("serialize");
        assert_eq!(output, input);
        let stable: Envelope = serde_json::from_value(output).expect("rehydrate");
        assert_eq!(serde_json::to_value(stable).unwrap(), input);
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "wide-integer-roundtrip-smoke"
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
                .join("target/wide-integer-roundtrip-smoke"),
        )
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .expect("run generated round-trip test");
    assert!(
        output.status.success(),
        "generated wide-integer round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
