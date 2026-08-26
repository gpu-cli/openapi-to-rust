//! Draft-04 positional `items` (issue #60).
//!
//! JSON Schema 2020-12 spells a tuple `prefixItems: [A, B]`, but tooling that
//! predates it still emits the draft-04 positional form `items: [A, B]` under
//! `openapi: "3.1.0"` — FastAPI/pydantic v1 does. The generator used to reject
//! those documents outright with "data did not match any variant of untagged
//! enum Schema" and no indication of where the offending node lived.

use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::openapi::{Items, Schema};
use openapi_to_rust::server::codegen::ServerCodegen;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn spec_with_pair_keyword(keyword: &str) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "repro", "version": "1.0.0" },
        "paths": { "/example": { "post": {
            "operationId": "example",
            "requestBody": { "required": true, "content": {
                "application/json": { "schema": { "$ref": "#/components/schemas/Body" } }
            }},
            "responses": { "200": { "description": "OK" } }
        }}},
        "components": { "schemas": { "Body": {
            "type": "object",
            "required": ["pair"],
            "properties": { "pair": {
                "type": "array",
                "minItems": 2,
                "maxItems": 2,
                keyword: [{ "type": "string" }, { "type": "string" }]
            }}
        }}}
    })
}

fn generate(spec: serde_json::Value) -> String {
    let mut analysis = SchemaAnalyzer::new(spec)
        .expect("spec parses")
        .analyze()
        .expect("spec analyzes");
    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("code generates")
}

#[test]
fn positional_items_generate_the_same_types_as_prefix_items() {
    let tuple_form = generate(spec_with_pair_keyword("items"));
    let canonical_form = generate(spec_with_pair_keyword("prefixItems"));

    assert!(
        tuple_form.contains("pub pair: Vec<serde_json::Value>"),
        "positional items must produce an array field, got:\n{tuple_form}"
    );
    assert_eq!(
        tuple_form, canonical_form,
        "`items: [A, B]` must generate exactly what `prefixItems: [A, B]` generates"
    );
}

#[test]
fn both_tuple_spellings_read_back_through_positional_items() {
    let tuple: Schema = serde_json::from_value(json!({
        "type": "array",
        "items": [{ "type": "string" }, { "type": "integer" }]
    }))
    .expect("draft-04 tuple parses");
    let canonical: Schema = serde_json::from_value(json!({
        "type": "array",
        "prefixItems": [{ "type": "string" }, { "type": "integer" }]
    }))
    .expect("2020-12 tuple parses");
    let single: Schema = serde_json::from_value(json!({
        "type": "array",
        "items": { "type": "string" }
    }))
    .expect("single-schema items parses");

    assert_eq!(tuple.details().positional_items().map(<[_]>::len), Some(2));
    assert_eq!(
        canonical.details().positional_items().map(<[_]>::len),
        Some(2)
    );
    assert!(tuple.details().item_schema().is_none());
    assert!(single.details().positional_items().is_none());
    assert!(matches!(single.details().items, Some(Items::Single(_))));
}

#[test]
fn positional_items_reach_the_server_validator_as_prefix_items() {
    let spec = spec_with_pair_keyword("items");
    let analysis = SchemaAnalyzer::new(spec)
        .expect("spec parses")
        .analyze()
        .expect("spec analyzes");
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["example".into()],
        prune_models: true,
        validation: ServerValidationSection::default(),
    };
    let config = GeneratorConfig {
        enable_async_client: false,
        server: Some(server.clone()),
        ..Default::default()
    };
    let files = ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .expect("server generates");
    let validation = files
        .iter()
        .find(|file| file.path.ends_with("validation.rs"))
        .expect("validation module is emitted");

    // A 2020-12 validator ignores an array-valued `items`, so the tuple must be
    // rewritten or the positions would go unchecked at runtime.
    assert!(
        validation.content.contains("prefixItems"),
        "embedded bundle must carry the canonical spelling:\n{}",
        validation.content
    );
    assert!(
        !validation.content.contains(r#"\"items\":["#),
        "embedded bundle must not keep the draft-04 spelling:\n{}",
        validation.content
    );
}
