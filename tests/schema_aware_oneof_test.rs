use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn schema_aware_oneof_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "schema-aware oneOf", "version": "1" },
        "paths": {},
        "components": { "schemas": {
            "PermissiveToken": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "access_token": { "type": ["string", "null"] },
                    "channel_id": { "type": ["string", "null"] }
                }
            },
            "Webhook": {
                "type": "object",
                "additionalProperties": false,
                "required": ["url"],
                "properties": { "url": { "type": "string" } }
            },
            "Connection": {
                "oneOf": [
                    { "$ref": "#/components/schemas/PermissiveToken" },
                    { "$ref": "#/components/schemas/Webhook" }
                ]
            },
            "ConnectionReversed": {
                "oneOf": [
                    { "$ref": "#/components/schemas/Webhook" },
                    { "$ref": "#/components/schemas/PermissiveToken" }
                ]
            },
            "OverlapA": {
                "type": "object",
                "additionalProperties": false,
                "required": ["shared"],
                "properties": {
                    "shared": { "type": "string" },
                    "a": { "type": "string" }
                }
            },
            "OverlapB": {
                "type": "object",
                "additionalProperties": false,
                "required": ["shared"],
                "properties": {
                    "shared": { "type": "string" },
                    "b": { "type": "string" }
                }
            },
            "ExclusiveOverlap": {
                "oneOf": [
                    { "$ref": "#/components/schemas/OverlapA" },
                    { "$ref": "#/components/schemas/OverlapB" }
                ]
            },
            "NonExclusiveOverlap": {
                "anyOf": [
                    { "$ref": "#/components/schemas/OverlapA" },
                    { "$ref": "#/components/schemas/OverlapB" }
                ]
            },
            "AnyPermissive": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" } }
            },
            "AnyDetailed": {
                "type": "object",
                "required": ["id", "detail"],
                "properties": {
                    "id": { "type": "string" },
                    "detail": { "type": "string" }
                }
            },
            "LosslessAny": {
                "anyOf": [
                    { "$ref": "#/components/schemas/AnyPermissive" },
                    { "$ref": "#/components/schemas/AnyDetailed" }
                ]
            },
            "NumericKindA": {
                "type": "object",
                "required": ["type", "value"],
                "properties": {
                    "type": { "type": "integer", "enum": [1] },
                    "value": { "type": "string" }
                }
            },
            "NumericKindB": {
                "type": "object",
                "required": ["type", "value"],
                "properties": {
                    "type": { "type": "integer", "enum": [2] },
                    "value": { "type": "string" }
                }
            },
            "NumericKind": {
                "oneOf": [
                    { "$ref": "#/components/schemas/NumericKindA" },
                    { "$ref": "#/components/schemas/NumericKindB" }
                ]
            },
            "RequiredUser": {
                "type": "object",
                "required": ["login"],
                "properties": { "login": { "type": "string" } }
            },
            "ClosedEmpty": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "UserOrEmpty": {
                "oneOf": [
                    { "$ref": "#/components/schemas/RequiredUser" },
                    { "$ref": "#/components/schemas/ClosedEmpty" }
                ]
            },
            "ConflictingKindBase": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["base"] }
                }
            },
            "ConflictingKindNarrow": {
                "allOf": [
                    { "$ref": "#/components/schemas/ConflictingKindBase" },
                    {
                        "type": "object",
                        "properties": {
                            "kind": { "type": "string", "enum": ["narrow"] }
                        }
                    }
                ]
            },
            "Marker": {
                "type": "object",
                "additionalProperties": false,
                "required": ["marker"],
                "properties": { "marker": { "type": "string" } }
            },
            "EmptyDomainUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/ConflictingKindNarrow" },
                    { "$ref": "#/components/schemas/Marker" }
                ]
            },
            "NullableWeb": {
                "type": "object",
                "additionalProperties": false,
                "required": ["call_type"],
                "properties": {
                    "call_type": { "type": "string", "enum": ["web"] },
                    "storage": {
                        "type": "string",
                        "enum": ["everything"],
                        "nullable": true
                    }
                }
            },
            "NullablePhone": {
                "type": "object",
                "additionalProperties": false,
                "required": ["call_type"],
                "properties": {
                    "call_type": { "type": "string", "enum": ["phone"] }
                }
            },
            "NullableLiteralUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/NullableWeb" },
                    { "$ref": "#/components/schemas/NullablePhone" }
                ]
            }
        } }
    })
}

#[test]
fn oneof_analysis_retains_exclusivity_without_changing_anyof() {
    let analysis = SchemaAnalyzer::new(schema_aware_oneof_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");

    let SchemaType::Union { exclusive, .. } = &analysis.schemas["Connection"].schema_type else {
        panic!("Connection should be an untagged union");
    };
    assert!(*exclusive);

    let SchemaType::Union { exclusive, .. } = &analysis.schemas["NonExclusiveOverlap"].schema_type
    else {
        panic!("NonExclusiveOverlap should be an untagged union");
    };
    assert!(!*exclusive);
}

#[test]
fn generated_oneof_selects_one_complete_shape_independent_of_branch_order() {
    let mut analysis = SchemaAnalyzer::new(schema_aware_oneof_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("spec should generate");
    let compact = generated.split_whitespace().collect::<String>();
    assert!(compact.contains("impl<'de>Deserialize<'de>forConnection"));
    assert!(compact.contains("pubenumNonExclusiveOverlap"));

    generated.push_str(
        r#"
#[cfg(test)]
mod schema_aware_oneof_runtime {
    use super::{
        Connection, ConnectionReversed, EmptyDomainUnion, ExclusiveOverlap, LosslessAny,
        NonExclusiveOverlap, NullableLiteralUnion, NumericKind, UserOrEmpty,
    };

    #[test]
    fn complete_shape_wins_regardless_of_branch_order() {
        for input in [
            serde_json::json!({"url": "https://hooks.example.test/one"}),
            serde_json::json!({"access_token": null, "channel_id": "C123"}),
        ] {
            let forward: Connection = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(forward).unwrap(), input);
            let reversed: ConnectionReversed = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(reversed).unwrap(), input);
        }
        for input in [
            serde_json::json!({"type": 1, "value": "a"}),
            serde_json::json!({"type": 2, "value": "b"}),
        ] {
            let hydrated: NumericKind = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
        }
    }

    #[test]
    fn ambiguous_and_no_match_oneof_values_are_explicit_errors() {
        let ambiguous = serde_json::from_value::<ExclusiveOverlap>(
            serde_json::json!({"shared": "both"}),
        )
        .unwrap_err();
        assert!(ambiguous.to_string().contains("ambiguous oneOf value"));

        let no_match = serde_json::from_value::<Connection>(
            serde_json::json!({"unknown": true}),
        )
        .unwrap_err();
        assert!(no_match.to_string().contains("no oneOf branch"));
    }

    #[test]
    fn overlapping_anyof_keeps_non_exclusive_serde_behavior() {
        let input = serde_json::json!({"shared": "either"});
        let hydrated: NonExclusiveOverlap = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);

        let detailed = serde_json::json!({"id": "one", "detail": "preserved"});
        let hydrated: LosslessAny = serde_json::from_value(detailed.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), detailed);
    }

    #[test]
    fn closed_empty_object_is_not_an_unconstrained_value_branch() {
        for input in [serde_json::json!({}), serde_json::json!({"login": "octocat"})] {
            let hydrated: UserOrEmpty = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
        }

        let marker = serde_json::json!({"marker": "valid"});
        let hydrated: EmptyDomainUnion = serde_json::from_value(marker.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), marker);

        let nullable = serde_json::json!({"call_type": "web", "storage": null});
        let hydrated: NullableLiteralUnion =
            serde_json::from_value(nullable.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), nullable);
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "schema-aware-oneof-smoke"
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
                .join("target/schema-aware-oneof-smoke"),
        )
        .output()
        .expect("run generated oneOf test");
    assert!(
        output.status.success(),
        "generated schema-aware oneOf failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
