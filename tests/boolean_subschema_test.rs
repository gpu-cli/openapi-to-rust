//! Boolean subschemas (issue #63).
//!
//! JSON Schema 2020-12 allows `true` and `false` wherever a schema is allowed:
//! `true` accepts every value, `false` accepts none. `properties: {extra: true}`
//! is how a spec says "this key exists, any value". The parser modeled schemas
//! as objects only, so one boolean anywhere in a document failed the whole
//! thing with "data did not match any variant of untagged enum Schema" — the
//! same failure #60 was about, and the reason the vendored 2020-12 suite had 38
//! parse failures.
//!
//! Generated code cannot say more than `serde_json::Value` for either, so what
//! these tests pin is that the document parses, the surrounding fields keep
//! their types, and the census reports the boolean honestly rather than as a
//! defect.

use openapi_to_rust::analysis::{SchemaAnalysis, UntypedReason, UntypedVerdict};
use openapi_to_rust::openapi::Schema;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn analyze(spec: Value) -> SchemaAnalysis {
    SchemaAnalyzer::new(spec)
        .expect("spec parses")
        .analyze()
        .expect("spec analyzes")
}

fn generate(spec: Value) -> String {
    let mut analysis = analyze(spec);
    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("code generates")
}

fn spec_with_schemas(schemas: Value) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "boolean subschemas", "version": "1.0.0" },
        "components": { "schemas": schemas }
    })
}

#[test]
fn a_boolean_parses_in_every_subschema_position() {
    // One document exercising each position a boolean is legal in. Before this,
    // any one of them took the whole document down.
    let spec = spec_with_schemas(json!({
        "Everything": {
            "type": "object",
            "properties": { "anything": true, "forbidden": false },
            "patternProperties": { "^x-": true },
            "propertyNames": true,
            "additionalProperties": true,
            "contains": true,
            "not": false,
            "if": true,
            "then": true,
            "else": false,
            "unevaluatedProperties": true,
            "dependentSchemas": { "anything": true },
            "$defs": { "wide": true, "narrow": false }
        },
        "Items": { "type": "array", "items": true },
        "Tuple": {
            "type": "array",
            "prefixItems": [{ "type": "string" }, true],
            "items": false,
            "minItems": 2
        }
    }));

    let analysis = analyze(spec);
    assert!(analysis.schemas.contains_key("Everything"));
    assert!(analysis.schemas.contains_key("Tuple"));
}

#[test]
fn a_true_property_is_any_value_and_its_neighbours_keep_their_types() {
    let generated = generate(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": false,
            "properties": { "extra": true, "name": { "type": "string" } }
        }
    })));

    assert!(
        generated.contains("pub extra: Option<serde_json::Value>"),
        "`true` accepts any value:\n{generated}"
    );
    assert!(
        generated.contains("pub name: Option<String>"),
        "a boolean neighbour must not cost the other fields their types:\n{generated}"
    );
}

#[test]
fn a_boolean_is_reported_faithfully_rather_than_as_a_defect() {
    // Neither spelling loses type information: `true` declares an
    // unconstrained value and `false` declares one that cannot occur.
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": false,
            "properties": { "wide": true, "narrow": false }
        }
    })));

    let findings = analysis.untyped_fields();
    let reason_for = |context: &str| {
        findings
            .iter()
            .find(|finding| finding.context == context)
            .map(|finding| finding.reason)
    };
    assert_eq!(reason_for("Thing.wide"), Some(UntypedReason::AnySchema));
    assert_eq!(
        reason_for("Thing.narrow"),
        Some(UntypedReason::NeverMatches)
    );
    assert!(
        findings
            .iter()
            .all(|finding| finding.reason.verdict() == UntypedVerdict::Faithful),
        "a boolean schema is not a dropped type: {findings:?}"
    );
}

#[test]
fn a_true_branch_makes_a_union_unconstrained() {
    // `oneOf: [string, true]` admits everything, so there is no narrower type
    // than `serde_json::Value` — and no reason to emit a union.
    let generated = generate(spec_with_schemas(json!({
        "Loose": { "oneOf": [{ "type": "string" }, true] },
        "Holder": { "type": "object", "additionalProperties": false,
                    "properties": { "value": { "$ref": "#/components/schemas/Loose" } } }
    })));

    assert!(
        generated.contains("pub type Loose = serde_json::Value"),
        "a union containing `true` accepts anything:\n{generated}"
    );
}

#[test]
fn a_false_branch_can_never_be_taken_and_is_dropped() {
    let generated = generate(spec_with_schemas(json!({
        "Tight": { "oneOf": [{ "type": "string" }, false] },
        "Holder": { "type": "object", "additionalProperties": false,
                    "properties": { "value": { "$ref": "#/components/schemas/Tight" } } }
    })));

    assert!(
        generated.contains("pub type Tight = String"),
        "`oneOf: [A, false]` is `A`:\n{generated}"
    );
}

#[test]
fn booleans_round_trip_through_the_schema_model() {
    // The parse layer must not quietly rewrite them: a boolean schema
    // serializes back to the boolean it was.
    for value in [json!(true), json!(false)] {
        let schema: Schema = serde_json::from_value(value.clone()).expect("boolean parses");
        assert!(matches!(schema, Schema::Bool(_)));
        assert_eq!(serde_json::to_value(&schema).expect("serializes"), value);
    }
}

#[test]
fn count_keywords_accept_a_decimal_spelling() {
    // JSON Schema requires these to be non-negative integers but says nothing
    // about their spelling, so `maxItems: 2.0` is valid and appears in the
    // 2020-12 suite. Reading them as `u64` alone rejected the document.
    let schema: Schema = serde_json::from_value(json!({
        "type": "array",
        "minItems": 1.0,
        "maxItems": 2.0
    }))
    .expect("decimal counts parse");
    assert_eq!(schema.details().min_items, Some(1));
    assert_eq!(schema.details().max_items, Some(2));
}

#[test]
fn a_fractional_count_is_still_rejected() {
    // `maxItems: 2.5` is not a count in any spelling; accepting it would round
    // silently.
    let parsed: Result<Schema, _> =
        serde_json::from_value(json!({ "type": "array", "maxItems": 2.5 }));
    assert!(parsed.is_err(), "a fractional count must not be accepted");
}
