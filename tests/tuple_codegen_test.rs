//! Typed tuples for positional item schemas (issue #62).
//!
//! `prefixItems` (and the draft-04 `items: [A, B]` spelling from #60) carry the
//! element types and, when the spec pins the length, the arity. The generator
//! used to drop both and emit `Vec<serde_json::Value>`.
//!
//! The length is the load-bearing part: `prefixItems` on its own does NOT stop
//! an instance from carrying extra elements of any type, and a Rust tuple is
//! fixed-arity. Emitting a tuple for an open array would produce code that
//! compiles and then fails on payloads the spec permits, so the open cases here
//! assert the conservative fallback just as hard as the closed ones assert the
//! tuple.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn generate(spec: Value) -> String {
    let mut analysis = SchemaAnalyzer::new(spec)
        .expect("spec parses")
        .analyze()
        .expect("spec analyzes");
    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("code generates")
}

fn spec_with_pair(pair: Value) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "tuples", "version": "1.0.0" },
        "components": { "schemas": { "Body": {
            "type": "object",
            "required": ["pair"],
            "properties": { "pair": pair }
        }}}
    })
}

#[test]
fn exact_length_by_min_and_max_items_generates_a_tuple() {
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "minItems": 2,
        "maxItems": 2,
        "prefixItems": [{ "type": "string" }, { "type": "integer" }]
    })));

    assert!(
        generated.contains("pub pair: (String, i64)"),
        "expected a typed tuple, got:\n{generated}"
    );
}

#[test]
fn draft_04_positional_items_generate_the_same_tuple() {
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "minItems": 2,
        "maxItems": 2,
        "items": [{ "type": "string" }, { "type": "integer" }]
    })));

    assert!(
        generated.contains("pub pair: (String, i64)"),
        "the draft-04 spelling must generate what prefixItems generates, got:\n{generated}"
    );
}

#[test]
fn items_false_closes_a_tuple() {
    // The canonical 2020-12 way to say "no elements beyond the positions".
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "minItems": 2,
        "prefixItems": [{ "type": "string" }, { "type": "boolean" }],
        "items": false
    })));

    assert!(
        generated.contains("pub pair: (String, bool)"),
        "`items: false` must close the tuple, got:\n{generated}"
    );
}

#[test]
fn additional_items_false_closes_a_draft_04_tuple() {
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "minItems": 2,
        "items": [{ "type": "string" }, { "type": "boolean" }],
        "additionalItems": false
    })));

    assert!(
        generated.contains("pub pair: (String, bool)"),
        "`additionalItems: false` must close the tuple, got:\n{generated}"
    );
}

#[test]
fn open_prefix_items_stay_an_untyped_array() {
    // No length cap: ["a", 1, "anything", {}] is a valid instance, so a tuple
    // would fail to deserialize data the spec allows.
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "prefixItems": [{ "type": "string" }, { "type": "integer" }]
    })));

    assert!(
        generated.contains("pub pair: Vec<serde_json::Value>"),
        "an open array must not become a tuple, got:\n{generated}"
    );
}

#[test]
fn closed_variable_length_positions_of_one_type_become_a_vec() {
    // At most 2 elements, both strings, but as few as none: no fixed arity to
    // spell as a tuple, yet every element is a String.
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "maxItems": 2,
        "prefixItems": [{ "type": "string" }, { "type": "string" }]
    })));

    assert!(
        generated.contains("pub pair: Vec<String>"),
        "expected a typed vec, got:\n{generated}"
    );
}

#[test]
fn closed_variable_length_positions_of_mixed_types_stay_untyped() {
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "maxItems": 2,
        "prefixItems": [{ "type": "string" }, { "type": "integer" }]
    })));

    assert!(
        generated.contains("pub pair: Vec<serde_json::Value>"),
        "a variable-length heterogeneous array has no single element type, got:\n{generated}"
    );
}

#[test]
fn single_position_tuple_keeps_the_trailing_comma() {
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "minItems": 1,
        "maxItems": 1,
        "prefixItems": [{ "type": "string" }]
    })));

    // `(String)` is just `String`; serde would then expect a bare value where
    // the spec says one-element array.
    assert!(
        generated.contains("pub pair: (String,)"),
        "expected a one-element tuple, got:\n{generated}"
    );
}

#[test]
fn referenced_positions_keep_their_named_types() {
    let generated = generate(json!({
        "openapi": "3.1.0",
        "info": { "title": "tuples", "version": "1.0.0" },
        "components": { "schemas": {
            "Point": { "type": "object", "properties": { "x": { "type": "integer" } } },
            "Body": {
                "type": "object",
                "required": ["pair"],
                "properties": { "pair": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 2,
                    "prefixItems": [{ "$ref": "#/components/schemas/Point" }, { "type": "number" }]
                }}
            }
        }}
    }));

    assert!(
        generated.contains("pub pair: (Point, f64)"),
        "a $ref position must keep its generated type, got:\n{generated}"
    );
    assert!(
        generated.contains("pub struct Point"),
        "the referenced schema must still be generated, got:\n{generated}"
    );
}

#[test]
fn inline_object_positions_are_hoisted_to_named_types() {
    let generated = generate(spec_with_pair(json!({
        "type": "array",
        "minItems": 2,
        "maxItems": 2,
        "prefixItems": [
            { "type": "string" },
            { "type": "object", "properties": { "count": { "type": "integer" } } }
        ]
    })));

    assert!(
        generated.contains("pub pair: (String, BodyPairItem2)"),
        "an inline object position must hoist a named type, got:\n{generated}"
    );
    assert!(
        generated.contains("pub struct BodyPairItem2"),
        "the hoisted type must be generated, got:\n{generated}"
    );
    assert!(
        generated.contains("pub count"),
        "the hoisted type must keep its fields, got:\n{generated}"
    );
}

#[test]
fn a_named_tuple_schema_generates_a_type_alias() {
    let generated = generate(json!({
        "openapi": "3.1.0",
        "info": { "title": "tuples", "version": "1.0.0" },
        "components": { "schemas": {
            "Coordinate": {
                "type": "array",
                "description": "A latitude/longitude pair.",
                "minItems": 2,
                "maxItems": 2,
                "prefixItems": [{ "type": "number" }, { "type": "number" }]
            },
            "Body": {
                "type": "object",
                "required": ["at"],
                "properties": { "at": { "$ref": "#/components/schemas/Coordinate" } }
            }
        }}
    }));

    assert!(
        generated.contains("pub type Coordinate = (f64, f64)"),
        "a top-level tuple schema must alias to a tuple, got:\n{generated}"
    );
    assert!(
        generated.contains("pub at: Coordinate"),
        "references to it must use the alias, got:\n{generated}"
    );
}
