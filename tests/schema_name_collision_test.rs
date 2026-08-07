use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

#[test]
fn distinct_component_keys_that_map_to_the_same_rust_type_are_disambiguated() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "name-collision-poc", "version": "1.0.0" },
        "paths": {
            "/event": {
                "get": {
                    "operationId": "getEvent",
                    "responses": {
                        "200": {
                            "description": "one event",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/session.status" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "SessionStatus": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["idle"] }
                            },
                            "required": ["type"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["busy"] },
                                "detail": { "type": "string" }
                            },
                            "required": ["type"]
                        }
                    ]
                },
                "session.status": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "data": {
                            "type": "object",
                            "properties": {
                                "sessionID": { "type": "string" },
                                "status": { "$ref": "#/components/schemas/SessionStatus" }
                            },
                            "required": ["sessionID", "status"]
                        }
                    },
                    "required": ["id", "data"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("spec should parse");
    let mut analysis = analyzer.analyze().expect("spec should analyze");
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("colliding component schema names should be disambiguated");

    assert!(generated.contains("pub enum SessionStatus"));
    assert!(generated.contains("pub struct SessionStatus2"));
    assert!(generated.contains("pub status: SessionStatus"));
    assert!(analysis.schemas.contains_key("SessionStatus"));
    assert!(analysis.schemas.contains_key("SessionStatus2"));
    assert!(!analysis.schemas.contains_key("session.status"));
    assert_eq!(
        analysis.operations["getEvent"].response_schemas["200"],
        "SessionStatus2"
    );
}
