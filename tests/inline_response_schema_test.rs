//! Test inline response schema handling
//!
//! When an endpoint has an inline schema (not a $ref), the generator should still
//! produce a usable response type, not just `()`.

use openapi_to_rust::SchemaAnalyzer;

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
fn test_multiple_inline_responses_have_distinct_schemas() {
    // Regression test for issue #8: when an operation declares multiple responses
    // with inline schemas (e.g. 200 success + 400 error), each must produce a
    // distinct registered schema. Previously the inline naming function ignored
    // the status code, so the 400 schema overwrote the 200 in the schema registry
    // and the generated client tried to deserialize success bodies as the error
    // shape.
    let spec = serde_json::json!({
        "openapi": "3.1.0",
        "info": { "title": "Test API", "version": "1.0.0" },
        "paths": {
            "/todos": {
                "get": {
                    "operationId": "listTodos",
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "id": { "type": "string" },
                                                "title": { "type": "string" }
                                            },
                                            "required": ["id", "title"]
                                        }
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "Bad Request",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "error": { "type": "string" }
                                        },
                                        "required": ["error"]
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
        .get("listTodos")
        .expect("Operation not found");

    // Both status codes must be present.
    let s200 = op
        .response_schemas
        .get("200")
        .expect("200 response missing from response_schemas");
    let s400 = op
        .response_schemas
        .get("400")
        .expect("400 response missing from response_schemas");

    // The two responses must map to *distinct* registered schemas.
    assert_ne!(
        s200, s400,
        "200 and 400 inline response schemas collided into the same name: {s200}"
    );

    // Both names must actually exist in the schema registry.
    assert!(
        analysis.schemas.contains_key(s200),
        "200 schema {s200} not registered"
    );
    assert!(
        analysis.schemas.contains_key(s400),
        "400 schema {s400} not registered"
    );

    // Sanity: the registered schemas should differ structurally — the 200 is an
    // array wrapper / object with id+title, the 400 is an object with `error`.
    let analyzed_200 = &analysis.schemas[s200];
    let analyzed_400 = &analysis.schemas[s400];
    assert_ne!(
        format!("{:?}", analyzed_200.schema_type),
        format!("{:?}", analyzed_400.schema_type),
        "200 and 400 schemas resolved to the same SchemaType — collision not actually fixed"
    );
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
