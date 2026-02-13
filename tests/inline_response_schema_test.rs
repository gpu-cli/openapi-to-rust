//! Test inline response schema handling
//!
//! When an endpoint has an inline schema (not a $ref), the generator should still
//! produce a usable response type, not just `()`.

use openapi_generator::SchemaAnalyzer;

#[test]
fn test_inline_array_response_analyzed() {
    // OpenAPI spec with inline array response (like OpenCode's /skill endpoint)
    let spec = serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "paths": {
            "/skill": {
                "get": {
                    "operationId": "app.skills",
                    "summary": "Get skills",
                    "responses": {
                        "200": {
                            "description": "List of skills",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "name": { "type": "string" },
                                                "description": { "type": "string" }
                                            },
                                            "required": ["name", "description"]
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Placeholder": {
                    "type": "object",
                    "properties": { "id": { "type": "string" } }
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze");

    // The operation should have a response schema
    let op = analysis
        .operations
        .get("app.skills")
        .expect("Operation not found");

    // Before fix: response_schemas would be empty because there's no $ref
    // After fix: should have a generated type name for the inline schema
    insta::assert_debug_snapshot!(op.response_schemas);
}

#[test]
fn test_inline_object_response_analyzed() {
    // OpenAPI spec with inline object response
    let spec = serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "paths": {
            "/status": {
                "get": {
                    "operationId": "getStatus",
                    "responses": {
                        "200": {
                            "description": "Status info",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "healthy": { "type": "boolean" },
                                            "version": { "type": "string" }
                                        },
                                        "required": ["healthy", "version"]
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Placeholder": {
                    "type": "object",
                    "properties": { "id": { "type": "string" } }
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze");

    let op = analysis
        .operations
        .get("getStatus")
        .expect("Operation not found");

    insta::assert_debug_snapshot!(op.response_schemas);
}

#[test]
fn test_ref_response_still_works() {
    // Ensure we don't break the existing $ref handling
    let spec = serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "paths": {
            "/user": {
                "get": {
                    "operationId": "getUser",
                    "responses": {
                        "200": {
                            "description": "User details",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/User"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "name": { "type": "string" }
                    }
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze");

    let op = analysis
        .operations
        .get("getUser")
        .expect("Operation not found");

    // This should still work - "User" should be in response_schemas
    assert_eq!(op.response_schemas.get("200"), Some(&"User".to_string()));
}
