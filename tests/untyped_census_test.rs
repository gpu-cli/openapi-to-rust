//! The untyped-output census (`--report-untyped`).
//!
//! `serde_json::Value` in generated code means one of two very different
//! things: the schema declared an unconstrained value, or the generator failed
//! to carry through type information the schema had. The census exists to tell
//! them apart across a corpus, so the second kind can be found and fixed.
//!
//! Its one hard requirement is that it not lie: a reason must describe why that
//! specific field went untyped, and a fallback must never escape unreported.

use openapi_to_rust::analysis::{UntypedReason, UntypedShape, UntypedVerdict};
use openapi_to_rust::{SchemaAnalyzer, analysis::SchemaAnalysis};
use serde_json::{Value, json};

fn analyze(spec: Value) -> SchemaAnalysis {
    SchemaAnalyzer::new(spec)
        .expect("spec parses")
        .analyze()
        .expect("spec analyzes")
}

fn spec_with_schemas(schemas: Value) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "census", "version": "1.0.0" },
        "components": { "schemas": schemas }
    })
}

#[test]
fn a_fully_typed_spec_reports_nothing() {
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": false,
            "properties": { "name": { "type": "string" }, "count": { "type": "integer" } }
        }
    })));

    assert!(analysis.untyped_fields().is_empty());
}

#[test]
fn an_unconstrained_object_is_reported_as_faithful() {
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": false,
            "properties": { "meta": { "type": "object" } }
        }
    })));

    let findings = analysis.untyped_fields();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].context, "Thing.meta");
    assert_eq!(findings[0].shape, UntypedShape::Value);
    assert_eq!(findings[0].reason, UntypedReason::OpaqueObject);
    assert_eq!(findings[0].reason.verdict(), UntypedVerdict::Faithful);
}

#[test]
fn open_additional_properties_are_reported_on_the_owning_schema() {
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": true,
            "properties": { "name": { "type": "string" } }
        }
    })));

    let findings = analysis.untyped_fields();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].context, "Thing.<additionalProperties>");
    assert_eq!(findings[0].shape, UntypedShape::ValueMap);
    assert_eq!(
        findings[0].reason,
        UntypedReason::UntypedAdditionalProperties
    );
}

#[test]
fn an_array_without_items_is_reported_as_an_array_shape() {
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": false,
            "properties": { "tags": { "type": "array" } }
        }
    })));

    let findings = analysis.untyped_fields();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0].shape, UntypedShape::ValueArray);
    assert_eq!(findings[0].reason, UntypedReason::ArrayWithoutItems);
}

#[test]
fn nested_positions_keep_a_path_that_locates_them() {
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                // Hoisted to a named struct: typed, and not a finding.
                "rows": { "type": "array", "items": { "type": "object" } },
                // Nothing to hoist: the element itself is unconstrained.
                "raws": { "type": "array", "items": {} }
            }
        }
    })));

    let findings = analysis.untyped_fields();
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(
        findings[0].context, "Thing.raws[]",
        "the path must say the array's element is what went untyped"
    );
    assert_eq!(findings[0].reason, UntypedReason::AnySchema);
}

#[test]
fn a_reference_counts_once_per_use_not_once_per_schema() {
    // The whole point of deriving the census from analyzed types: one untyped
    // schema reached from three properties is three untyped generated fields.
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "a": { "type": "object" },
                "b": { "type": "object" },
                "c": { "type": "object" }
            }
        }
    })));

    assert_eq!(analysis.untyped_fields().len(), 3);
}

#[test]
fn no_fallback_escapes_the_taxonomy() {
    // Every reason must classify. `Unclassified` means a fallback reached the
    // normalization net without being named at its source, which is a gap to
    // close rather than a category to live with.
    let analysis = analyze(spec_with_schemas(json!({
        "Opaque": { "type": "object" },
        "Anything": {},
        "OpenMap": { "type": "object", "additionalProperties": true },
        "Bare": { "type": "array" },
        "Mixed": { "anyOf": [{ "type": "object" }, { "type": "array" }] },
        "Holder": {
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "opaque": { "$ref": "#/components/schemas/Opaque" },
                "anything": { "$ref": "#/components/schemas/Anything" },
                "map": { "$ref": "#/components/schemas/OpenMap" },
                "bare": { "$ref": "#/components/schemas/Bare" },
                "mixed": { "$ref": "#/components/schemas/Mixed" }
            }
        }
    })));

    let unclassified = analysis
        .untyped_fields()
        .into_iter()
        .filter(|finding| finding.reason == UntypedReason::Unclassified)
        .collect::<Vec<_>>();
    assert!(
        unclassified.is_empty(),
        "unnamed fallbacks reached the census: {unclassified:?}"
    );
}
