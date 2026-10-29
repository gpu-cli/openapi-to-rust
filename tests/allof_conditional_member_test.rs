//! An `allOf` member holding only `if`/`then`/`else` rules constrains values,
//! not shape, and an owner's own `properties` always make it an object
//! (gpu-cli/openapi-to-rust#111).

use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn utc_name_rule() -> Value {
    json!({
        "if": { "properties": { "zone": { "const": "utc" } }, "required": ["zone"] },
        "then": { "properties": { "name": { "pattern": "Z$" } } }
    })
}

fn spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "conditional allOf members", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "Pet": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/Cat" },
                        { "$ref": "#/components/schemas/Dog" }
                    ],
                    "discriminator": { "propertyName": "kind" }
                },
                "Cat": {
                    "type": "object",
                    "required": ["kind", "name"],
                    "properties": {
                        "kind": { "type": "string", "const": "Cat" },
                        "name": { "type": "string" },
                        "zone": { "type": "string" }
                    },
                    "allOf": [utc_name_rule()]
                },
                "Dog": {
                    "type": "object",
                    "required": ["kind", "name"],
                    "properties": {
                        "kind": { "type": "string", "const": "Dog" },
                        "name": { "type": "string" }
                    }
                },
                "InlinePet": {
                    "oneOf": [
                        {
                            "properties": {
                                "kind": { "type": "string", "const": "Bird" },
                                "wings": { "type": "integer" }
                            },
                            "required": ["kind"],
                            "allOf": [{ "if": { "required": ["wings"] }, "then": { "required": ["kind"] } }]
                        },
                        { "$ref": "#/components/schemas/Dog" }
                    ],
                    "discriminator": { "propertyName": "kind" }
                },
                "Base": {
                    "type": "object",
                    "required": ["id"],
                    "properties": { "id": { "type": "string" } }
                },
                "ChildWithOwnProperties": {
                    "type": "object",
                    "required": ["extra"],
                    "properties": { "extra": { "type": "string" } },
                    "allOf": [{ "$ref": "#/components/schemas/Base" }]
                },
                "OwnPropertiesWithRequiredMember": {
                    "type": "object",
                    "properties": { "a": { "type": "string" } },
                    "allOf": [{ "required": ["a"] }]
                },
                "BaseWithRule": {
                    "allOf": [
                        { "$ref": "#/components/schemas/Base" },
                        { "if": { "required": ["id"] }, "else": { "description": "unused" } }
                    ]
                }
            }
        }
    })
}

fn analyze() -> openapi_to_rust::SchemaAnalysis {
    SchemaAnalyzer::new(spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze")
}

fn object_fields(analysis: &openapi_to_rust::SchemaAnalysis, name: &str) -> Vec<String> {
    let SchemaType::Object { properties, .. } = &analysis.schemas[name].schema_type else {
        panic!(
            "{name} should be an object, got {:?}",
            analysis.schemas[name].schema_type
        );
    };
    properties.keys().cloned().collect()
}

#[test]
fn conditional_only_member_keeps_the_owner_object() {
    let analysis = analyze();
    assert_eq!(object_fields(&analysis, "Cat"), ["kind", "name", "zone"]);
}

#[test]
fn discriminated_union_with_conditional_branch_stays_tagged() {
    let analysis = analyze();
    for union in ["Pet", "InlinePet"] {
        assert!(
            matches!(
                analysis.schemas[union].schema_type,
                SchemaType::DiscriminatedUnion { ref discriminator_field, .. }
                    if discriminator_field == "kind"
            ),
            "{union} should stay discriminated on `kind`, got {:?}",
            analysis.schemas[union].schema_type
        );
    }
}

#[test]
fn owner_properties_survive_single_member_allof() {
    let analysis = analyze();
    assert_eq!(
        object_fields(&analysis, "ChildWithOwnProperties"),
        ["extra", "id"]
    );
    assert_eq!(
        object_fields(&analysis, "OwnPropertiesWithRequiredMember"),
        ["a"]
    );
}

#[test]
fn reference_with_conditional_sibling_is_an_alias() {
    let analysis = analyze();
    assert!(
        matches!(
            analysis.schemas["BaseWithRule"].schema_type,
            SchemaType::Reference { ref target } if target == "Base"
        ),
        "BaseWithRule should alias Base, got {:?}",
        analysis.schemas["BaseWithRule"].schema_type
    );
}

#[test]
fn generated_code_matches_schema_without_the_conditional() {
    let generate = |spec: Value| {
        let mut analysis = SchemaAnalyzer::new(spec)
            .expect("spec should parse")
            .analyze()
            .expect("spec should analyze");
        CodeGenerator::new(GeneratorConfig {
            module_name: "conditional_allof".into(),
            enable_async_client: false,
            ..Default::default()
        })
        .generate(&mut analysis)
        .expect("spec should generate")
    };

    let with_rule = generate(spec());
    let mut without_rule = spec();
    without_rule["components"]["schemas"]["Cat"]
        .as_object_mut()
        .expect("Cat is an object")
        .remove("allOf");

    assert!(
        !with_rule.contains("pub type Cat = serde_json::Value"),
        "Cat must not become opaque. Code:\n{with_rule}"
    );
    assert_eq!(with_rule, generate(without_rule));
}
