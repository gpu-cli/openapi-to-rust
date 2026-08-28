use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn reference_sibling_spec() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "nullable reference siblings", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "Reason": {
                    "type": "string",
                    "enum": ["stop", "length"]
                },
                "ReasonAlias": { "$ref": "#/components/schemas/Reason" },
                "Envelope": {
                    "type": "object",
                    "required": ["required_nullable", "required_plain"],
                    "properties": {
                        "required_nullable": {
                            "$ref": "#/components/schemas/ReasonAlias",
                            "nullable": true
                        },
                        "required_plain": {
                            "$ref": "#/components/schemas/ReasonAlias",
                            "nullable": false
                        },
                        "optional_nullable": {
                            "$ref": "#/components/schemas/Reason",
                            "nullable": true
                        }
                    }
                },
                "NullableReasonList": {
                    "type": "array",
                    "items": {
                        "$ref": "#/components/schemas/ReasonAlias",
                        "nullable": true
                    }
                },
                "MinCount": {
                    "type": "object",
                    "additionalProperties": false,
                    "minProperties": 2,
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string" },
                        "note": { "type": "string", "nullable": true }
                    }
                },
                "MaxCount": {
                    "type": "object",
                    "additionalProperties": false,
                    "maxProperties": 1,
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string" },
                        "note": { "type": "string", "nullable": true }
                    }
                }
            }
        }
    })
}

#[test]
fn analyzer_and_generator_honor_local_nullable_reference_siblings() {
    let analysis = SchemaAnalyzer::new(reference_sibling_spec())
        .expect("reference-sibling spec should parse")
        .analyze()
        .expect("reference-sibling spec should analyze");
    let SchemaType::Object {
        properties,
        required,
        ..
    } = &analysis.schemas["Envelope"].schema_type
    else {
        panic!("Envelope should be an object");
    };
    assert!(required.contains("required_nullable"));
    assert!(properties["required_nullable"].nullable);
    assert!(!properties["required_plain"].nullable);
    assert!(properties["optional_nullable"].nullable);

    let SchemaType::Array { item_type } = &analysis.schemas["NullableReasonList"].schema_type
    else {
        panic!("NullableReasonList should be an array");
    };
    assert!(matches!(
        item_type.as_ref(),
        SchemaType::Nullable { inner_type }
            if matches!(inner_type.as_ref(), SchemaType::Reference { target } if target == "ReasonAlias")
    ));

    let mut generated_analysis = analysis;
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut generated_analysis)
        .expect("nullable reference siblings should generate");
    let compact = generated.split_whitespace().collect::<String>();
    assert!(compact.contains("pubrequired_nullable:Option<ReasonAlias>"));
    assert!(compact.contains("pubrequired_plain:ReasonAlias"));
    assert!(compact.contains("puboptional_nullable:Option<Option<Reason>>"));
    assert!(compact.contains("pubtypeNullableReasonList=Vec<Option<ReasonAlias>>"));

    assert!(
        !compact.contains("#[serde(skip_serializing_if=\"Option::is_none\")]pubrequired_nullable"),
        "required nullable references must serialize None as JSON null:\n{generated}"
    );
}

#[test]
fn generated_required_nullable_reference_round_trips_explicit_null() {
    let mut analysis = SchemaAnalyzer::new(reference_sibling_spec())
        .expect("reference-sibling spec should parse")
        .analyze()
        .expect("reference-sibling spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("nullable reference siblings should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod nullable_reference_sibling_roundtrip {
    use super::{Envelope, MaxCount, MinCount};

    #[test]
    fn required_null_is_present_and_stable() {
        let input = serde_json::json!({
            "required_nullable": null,
            "required_plain": "stop"
        });
        let hydrated: Envelope = serde_json::from_value(input.clone()).expect("hydrate null");
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);

        let non_null = serde_json::json!({
            "required_nullable": "length",
            "required_plain": "stop"
        });
        let hydrated: Envelope = serde_json::from_value(non_null.clone()).expect("hydrate value");
        assert_eq!(serde_json::to_value(hydrated).unwrap(), non_null);

        let missing_optional = serde_json::json!({
            "required_nullable": null,
            "required_plain": "stop"
        });
        let hydrated: Envelope = serde_json::from_value(missing_optional.clone()).unwrap();
        assert!(hydrated.optional_nullable.is_none());
        assert_eq!(serde_json::to_value(hydrated).unwrap(), missing_optional);

        let explicit_optional_null = serde_json::json!({
            "required_nullable": null,
            "required_plain": "stop",
            "optional_nullable": null
        });
        let hydrated: Envelope = serde_json::from_value(explicit_optional_null.clone()).unwrap();
        assert_eq!(hydrated.optional_nullable, Some(None));
        assert_eq!(serde_json::to_value(hydrated).unwrap(), explicit_optional_null);

        let optional_value = serde_json::json!({
            "required_nullable": null,
            "required_plain": "stop",
            "optional_nullable": "length"
        });
        let hydrated: Envelope = serde_json::from_value(optional_value.clone()).unwrap();
        assert!(matches!(hydrated.optional_nullable, Some(Some(_))));
        assert_eq!(serde_json::to_value(hydrated).unwrap(), optional_value);

        let min_properties = serde_json::json!({"id": "minimum", "note": null});
        let hydrated: MinCount = serde_json::from_value(min_properties.clone()).unwrap();
        assert_eq!(hydrated.note, Some(None));
        assert_eq!(serde_json::to_value(hydrated).unwrap(), min_properties);

        let max_properties = serde_json::json!({"id": "maximum"});
        let hydrated: MaxCount = serde_json::from_value(max_properties.clone()).unwrap();
        assert!(hydrated.note.is_none());
        assert_eq!(serde_json::to_value(hydrated).unwrap(), max_properties);
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "nullable-reference-sibling-roundtrip-smoke"
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
                .join("target/nullable-reference-sibling-roundtrip-smoke"),
        )
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .expect("run generated round-trip test");
    assert!(
        output.status.success(),
        "generated nullable-reference round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
