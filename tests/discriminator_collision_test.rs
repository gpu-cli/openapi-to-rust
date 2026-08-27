use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn generate(schemas: Value) -> String {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "discriminator-collision", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": schemas }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("spec should parse");
    let mut analysis = analyzer.analyze().expect("spec should analyze");
    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("spec should generate")
}

#[test]
fn repeated_implicit_discriminator_values_fall_back_to_untagged_union() {
    let generated = generate(json!({
        "GlobalEvent": {
            "type": "object",
            "properties": {
                "payload": {
                    "anyOf": [
                        { "$ref": "#/components/schemas/SyncEventSessionCreated" },
                        { "$ref": "#/components/schemas/SyncEventSessionUpdated" }
                    ]
                }
            },
            "required": ["payload"]
        },
        "SyncEventSessionCreated": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "type": { "type": "string", "enum": ["sync"] },
                "syncEvent": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": ["session.created.1"] },
                        "data": { "type": "string" }
                    },
                    "required": ["type", "data"]
                }
            },
            "required": ["id", "type", "syncEvent"]
        },
        "SyncEventSessionUpdated": {
            "type": "object",
            "properties": {
                "id": { "type": "string" },
                "type": { "type": "string", "enum": ["sync"] },
                "syncEvent": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": ["session.updated.1"] },
                        "data": { "type": "string" }
                    },
                    "required": ["type", "data"]
                }
            },
            "required": ["id", "type", "syncEvent"]
        }
    }));

    assert!(generated.contains("#[serde(untagged)]\npub enum GlobalEventPayload"));
    assert!(!generated.contains("#[serde(tag = \"type\")]\npub enum GlobalEventPayload"));
    assert_eq!(generated.matches("#[serde(rename = \"sync\")]").count(), 2);
    assert!(generated.contains("pub r#type: SyncEventSessionCreatedTypeSync"));
    assert!(generated.contains("pub r#type: SyncEventSessionUpdatedTypeSync"));
}

#[test]
fn unique_implicit_discriminator_values_still_generate_discriminated_union() {
    let generated = generate(json!({
        "Event": {
            "anyOf": [
                { "$ref": "#/components/schemas/Created" },
                { "$ref": "#/components/schemas/Updated" }
            ]
        },
        "Created": {
            "type": "object",
            "properties": {
                "type": { "type": "string", "enum": ["created"] },
                "id": { "type": "string" }
            },
            "required": ["type", "id"]
        },
        "Updated": {
            "type": "object",
            "properties": {
                "type": { "type": "string", "enum": ["updated"] },
                "id": { "type": "string" }
            },
            "required": ["type", "id"]
        }
    }));

    assert!(generated.contains("pub enum Event"));
    assert!(generated.contains("match discriminator.as_str()"));
    assert!(!generated.contains("#[serde(untagged)]\npub enum Event"));
}
