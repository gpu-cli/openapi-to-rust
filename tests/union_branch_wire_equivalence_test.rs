//! Union deserializers keep a branch only when re-serializing it reproduces
//! the input. Two encodings of the same value failed that check:
//!
//! - `"ttl": 1` decoded into an `f64` field re-serializes as `1.0`, and
//! - `"comment": null` decoded into an `Option` field is skipped on output.
//!
//! Neither loses information, but together they rejected every record of the
//! Cloudflare DNS API, which types integers as `number` and returns explicit
//! nulls for optional fields. This fixture is that shape, reduced: a `oneOf`
//! of record types keyed by an enum `type`, wrapped in an `allOf` with the
//! shared response fields.
//!
//! The negative cases matter as much: a branch that states `integer` must
//! still beat one that states `number` for `1`, and `1.5` must still reach the
//! `number` branch, so the looser comparison is only a fallback when no branch
//! reproduces the input exactly.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::process::Command;

fn spec() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "wire equivalence", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            "ARecord": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["A"] },
                    "content": { "type": "string" },
                    "ttl": { "type": "number" },
                    "comment": { "type": "string" }
                }
            },
            "MXRecord": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["MX"] },
                    "content": { "type": "string" },
                    "ttl": { "type": "number" },
                    "priority": { "type": "number" },
                    "comment": { "type": "string" }
                }
            },
            "Record": {
                "oneOf": [
                    { "$ref": "#/components/schemas/ARecord" },
                    { "$ref": "#/components/schemas/MXRecord" }
                ]
            },
            "RecordResponse": {
                "type": "object",
                "allOf": [
                    { "anyOf": [ { "$ref": "#/components/schemas/Record" } ] },
                    {
                        "type": "object",
                        "required": ["id"],
                        "properties": { "id": { "type": "string" } }
                    }
                ]
            },
            "IntegerCount": {
                "type": "object", "required": ["count"],
                "properties": { "count": { "type": "integer" } }
            },
            "NumberCount": {
                "type": "object", "required": ["count"],
                "properties": { "count": { "type": "number" } }
            },
            "Count": {
                "oneOf": [
                    { "$ref": "#/components/schemas/IntegerCount" },
                    { "$ref": "#/components/schemas/NumberCount" }
                ]
            },
            "DefaultedBranch": {
                "type": "object", "required": ["mode"],
                "properties": { "mode": { "type": "string", "default": "active" } }
            },
            "OtherBranch": {
                "type": "object", "required": ["id"],
                "properties": { "id": { "type": "string" } }
            },
            "DefaultedUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/DefaultedBranch" },
                    { "$ref": "#/components/schemas/OtherBranch" }
                ]
            },
            "DefaultedAny": {
                "anyOf": [
                    { "$ref": "#/components/schemas/DefaultedBranch" },
                    { "$ref": "#/components/schemas/OtherBranch" }
                ]
            },
            "NullableAnyResponse": {
                "type": "object", "required": ["result"],
                "properties": { "result": {
                    "anyOf": [
                        { "type": "object", "nullable": true },
                        { "type": "string", "nullable": true }
                    ]
                } }
            }
        } }
    })
}

#[test]
fn equivalent_wire_encodings_select_a_union_branch() {
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    let mut analyzer = SchemaAnalyzer::new(spec()).expect("parse wire equivalence spec");
    let mut analysis = analyzer.analyze().expect("analyze wire equivalence spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: temp.path().join("src/generated"),
        module_name: "wire_equivalence".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator
        .generate_all(&mut analysis)
        .expect("generate wire equivalence models");
    generator.write_files(&result).expect("write models");

    let dependency_fragment =
        std::fs::read_to_string(temp.path().join("src/generated/REQUIRED_DEPS.toml"))
            .expect("generated dependency fragment");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "wire-equivalence-smoke"
version = "0.0.0"
edition = "2024"
publish = false

{dependency_fragment}
"#
        ),
    )
    .expect("write scratch manifest");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::{Count, DefaultedAny, DefaultedUnion, NullableAnyResponse, Record, RecordResponse};
    use serde_json::json;

    #[test]
    fn an_integer_in_a_number_field_selects_its_branch() {
        let record: Record = serde_json::from_value(
            json!({"type": "A", "content": "192.0.2.1", "ttl": 1}),
        )
        .expect("`ttl: 1` must not be rejected because `f64` re-encodes it as `1.0`");
        assert!(matches!(record, Record::ARecord(_)));

        let record: Record = serde_json::from_value(
            json!({"type": "MX", "content": "mx.example.com", "ttl": 3600, "priority": 10}),
        )
        .expect("integer-valued numbers must select the MX branch");
        assert!(matches!(record, Record::MXRecord(_)));
    }

    #[test]
    fn an_explicit_null_for_an_optional_field_selects_its_branch() {
        let record: Record = serde_json::from_value(
            json!({"type": "A", "content": "192.0.2.1", "ttl": 3600, "comment": null}),
        )
        .expect("`comment: null` must not be rejected because `None` is skipped on output");
        assert!(matches!(record, Record::ARecord(_)));
    }

    #[test]
    fn a_full_response_with_both_encodings_decodes() {
        let response: RecordResponse = serde_json::from_value(json!({
            "id": "372e67954025e0ba6aaa6d586b9e0b59",
            "type": "MX", "content": "mx.example.com", "ttl": 1, "priority": 10,
            "comment": null
        }))
        .expect("the Cloudflare-shaped response must decode");
        assert_eq!(response.id, "372e67954025e0ba6aaa6d586b9e0b59");
    }

    #[test]
    fn an_exact_match_still_beats_an_equivalent_one() {
        let count: Count = serde_json::from_value(json!({"count": 1}))
            .expect("an integer count must still decode");
        assert!(
            matches!(count, Count::IntegerCount(_)),
            "the `integer` branch reproduces `1` exactly and must win: {count:?}"
        );

        let count: Count = serde_json::from_value(json!({"count": 1.5}))
            .expect("a fractional count must still decode");
        assert!(
            matches!(count, Count::NumberCount(_)),
            "only the `number` branch can hold `1.5`: {count:?}"
        );
    }

    #[test]
    fn an_added_output_key_is_not_wire_equivalent() {
        // A required property with a schema default gets #[serde(default)].
        // It can deserialize an absent property, then serializes it back as
        // an added key. That is neither of the wire differences allowed by
        // the oneOf fallback.
        let result = serde_json::from_value::<DefaultedUnion>(json!({}));
        assert!(result.is_err(), "a branch must not add a missing required key: {result:?}");

        let input = json!({"comment": null});
        assert!(
            serde_json::from_value::<DefaultedUnion>(input.clone()).is_err(),
            "a null input field must not hide an added output key in oneOf"
        );
        assert!(
            serde_json::from_value::<DefaultedAny>(input).is_err(),
            "a null input field must not hide an added output key in anyOf"
        );

        let exact: DefaultedUnion = serde_json::from_value(json!({"mode": "active"}))
            .expect("an exact defaulted branch still decodes");
        assert!(matches!(exact, DefaultedUnion::DefaultedBranch(_)));
    }

    #[test]
    fn a_required_nullable_anyof_property_roundtrips_null() {
        let input = json!({"result": null});
        let response: NullableAnyResponse = serde_json::from_value(input.clone())
            .expect("a nullable anyOf branch admits null");
        assert!(response.result.is_none());
        assert_eq!(serde_json::to_value(response).unwrap(), input);
    }
}
"#,
    )
    .expect("write scratch tests");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/wire-equivalence-smoke"),
        )
        .output()
        .expect("run generated wire equivalence tests");
    assert!(
        output.status.success(),
        "generated unions must accept equivalent wire encodings:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
