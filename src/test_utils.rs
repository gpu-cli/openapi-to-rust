use serde_json::json;

/// Create a minimal OpenAPI spec for testing
pub fn create_test_spec(schemas: serde_json::Value) -> serde_json::Value {
    json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": schemas
        }
    })
}

/// Create a schema with anyOf content field (like InputMessage)
pub fn create_content_union_schema() -> serde_json::Value {
    json!({
        "TestMessage": {
            "type": "object",
            "properties": {
                "role": {
                    "type": "string"
                },
                "content": {
                    "anyOf": [
                        {"type": "string"},
                        {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/ContentBlock"}
                        }
                    ]
                }
            }
        },
        "ContentBlock": {
            "type": "object",
            "properties": {
                "type": {"type": "string"},
                "text": {"type": "string"}
            }
        }
    })
}

/// Create a schema with property names containing underscores
pub fn create_underscore_property_schema() -> serde_json::Value {
    json!({
        "ConfigSchema": {
            "type": "object",
            "properties": {
                "allowed_tools": {
                    "anyOf": [
                        {"type": "null"},
                        {
                            "type": "array",
                            "items": {"type": "string"}
                        }
                    ]
                },
                "cache_control": {
                    "anyOf": [
                        {"type": "null"},
                        {"$ref": "#/components/schemas/CacheControl"}
                    ]
                }
            }
        },
        "CacheControl": {
            "type": "object",
            "properties": {
                "type": {"type": "string"}
            }
        }
    })
}

/// Create a schema with nested underscore names (like BetaListResponse_MessageBatch)
pub fn create_nested_underscore_schema() -> serde_json::Value {
    json!({
        "BetaListResponse_MessageBatch": {
            "type": "object",
            "properties": {
                "last_id": {
                    "anyOf": [
                        {"type": "null"},
                        {"type": "string"}
                    ]
                },
                "first_id": {
                    "anyOf": [
                        {"type": "null"},
                        {"type": "string"}
                    ]
                }
            }
        }
    })
}

/// Create a schema with duplicate enum variants
pub fn create_duplicate_variant_schema() -> serde_json::Value {
    json!({
        "ContentUnion": {
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "type": {"const": "text"}
                    }
                },
                {
                    "type": "object", 
                    "properties": {
                        "type": {"const": "text"}  // Duplicate!
                    }
                }
            ],
            "discriminator": {
                "propertyName": "type"
            }
        }
    })
}