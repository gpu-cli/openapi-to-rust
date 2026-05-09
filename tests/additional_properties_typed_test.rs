//! Q2.3: typed `BTreeMap<String, T>` from
//! `additionalProperties: <schema>` (default on; opt out via
//! `[generator.types.shape] additional_properties_typed = false`).

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
    type_mapping::TypeShapeConfig,
};
use serde_json::json;

fn ap_spec(value_schema: serde_json::Value) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "additionalProperties": value_schema
                }
            }
        }
    })
}

fn generate(spec: serde_json::Value, mapper: TypeMapper) -> String {
    let mut analyzer = SchemaAnalyzer::with_type_mapper(spec, mapper).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        ..Default::default()
    };
    CodeGenerator::new(cfg)
        .generate(&mut analysis)
        .expect("generate")
}

#[test]
fn ap_string_schema_default_emits_typed_btreemap() {
    let code = generate(
        ap_spec(json!({ "type": "string" })),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, String>"),
        "additionalProperties: <string schema> should produce BTreeMap<String, String>. Code:\n{code}"
    );
}

#[test]
fn ap_integer_schema_default_emits_typed_btreemap() {
    let code = generate(
        ap_spec(json!({ "type": "integer", "format": "int32" })),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, i32>"),
        "additionalProperties with int32 should produce BTreeMap<String, i32>. Code:\n{code}"
    );
}

#[test]
fn ap_typed_default_picks_up_format_typed_scalars() {
    // The value-type analysis should respect TypeMapper format
    // strategies, so additionalProperties: { format: uuid } yields
    // BTreeMap<String, uuid::Uuid>.
    let code = generate(
        ap_spec(json!({ "type": "string", "format": "uuid" })),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, uuid::Uuid>"),
        "additionalProperties with format: uuid should produce BTreeMap<String, uuid::Uuid>. Code:\n{code}"
    );
}

#[test]
fn ap_boolean_true_remains_untyped() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "additionalProperties": true
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains(
            "pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>"
        ),
        "additionalProperties: true should still produce BTreeMap<String, serde_json::Value>. Code:\n{code}"
    );
}

#[test]
fn ap_boolean_false_emits_no_field() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        !code.contains("pub additional_properties:"),
        "additionalProperties: false must not emit a field. Code:\n{code}"
    );
}

#[test]
fn ap_typed_off_falls_back_to_untyped() {
    let mut cfg = TypeMappingConfig::default();
    cfg.shape = Some(TypeShapeConfig {
        additional_properties_typed: Some(false),
        ..Default::default()
    });
    let code = generate(ap_spec(json!({ "type": "string" })), TypeMapper::new(cfg));
    assert!(
        code.contains(
            "pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>"
        ),
        "additional_properties_typed = false should degrade to serde_json::Value. Code:\n{code}"
    );
}

#[test]
fn ap_schema_ref_emits_btreemap_of_named_type() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Item": {
                    "type": "object",
                    "required": ["name"],
                    "properties": { "name": { "type": "string" } }
                },
                "Bag": {
                    "type": "object",
                    "additionalProperties": { "$ref": "#/components/schemas/Item" }
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, Item>"),
        "additionalProperties: $ref should produce BTreeMap<String, NamedType>. Code:\n{code}"
    );
}
