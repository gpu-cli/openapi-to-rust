#![cfg(feature = "test-helpers")]

//! Tests to ensure we minimize serde_json::Value usage in generated code
//!
//! These tests track and verify that we properly generate typed structures
//! instead of falling back to serde_json::Value where possible.

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn test_discriminated_union_with_discriminator_mapping() {
    // This tests the most common pattern where we currently fall back to serde_json::Value
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "MessageResult": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/SucceededResult"},
                        {"$ref": "#/components/schemas/ErroredResult"},
                        {"$ref": "#/components/schemas/CanceledResult"}
                    ],
                    "discriminator": {
                        "propertyName": "type",
                        "mapping": {
                            "succeeded": "#/components/schemas/SucceededResult",
                            "errored": "#/components/schemas/ErroredResult",
                            "canceled": "#/components/schemas/CanceledResult"
                        }
                    }
                },
                "SucceededResult": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "succeeded"},
                        "message": {"$ref": "#/components/schemas/Message"}
                    },
                    "required": ["type", "message"]
                },
                "ErroredResult": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "errored"},
                        "error": {"type": "string"}
                    },
                    "required": ["type", "error"]
                },
                "CanceledResult": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "canceled"},
                        "reason": {"type": "string"}
                    },
                    "required": ["type", "reason"]
                },
                "Message": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "required": ["id", "content"]
                }
            }
        }
    });

    let result = test_generation("discriminated_union_test", spec).expect("Generation failed");

    // The discriminated union should be generated as an enum, not serde_json::Value
    assert!(
        result.contains("pub enum MessageResult"),
        "MessageResult should be generated as an enum"
    );

    // Should not contain serde_json::Value in the MessageResult definition
    assert!(
        !result.contains("MessageResult(serde_json::Value)"),
        "MessageResult enum should not contain serde_json::Value"
    );

    // Verify the enum has proper variants - check for different possible formats
    assert!(
        result.contains("SucceededResult") || result.contains("Succeeded"),
        "Should contain SucceededResult variant"
    );
    assert!(
        result.contains("ErroredResult") || result.contains("Errored"),
        "Should contain ErroredResult variant"
    );
    assert!(
        result.contains("CanceledResult") || result.contains("Canceled"),
        "Should contain CanceledResult variant"
    );
}

#[test]
fn test_content_block_delta_union() {
    // Test the ContentBlockDelta pattern which has multiple variant types
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ContentBlockDelta": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/TextDelta"},
                        {"$ref": "#/components/schemas/InputJsonDelta"}
                    ],
                    "discriminator": {
                        "propertyName": "type",
                        "mapping": {
                            "text_delta": "#/components/schemas/TextDelta",
                            "input_json_delta": "#/components/schemas/InputJsonDelta"
                        }
                    }
                },
                "TextDelta": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "text_delta"},
                        "text": {"type": "string"}
                    },
                    "required": ["type", "text"]
                },
                "InputJsonDelta": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "input_json_delta"},
                        "partial_json": {"type": "string"}
                    },
                    "required": ["type", "partial_json"]
                }
            }
        }
    });

    let result =
        test_generation("content_block_delta_union_test", spec).expect("Generation failed");

    assert!(
        result.contains("pub enum ContentBlockDelta"),
        "ContentBlockDelta should be generated as an enum"
    );

    // Should not use serde_json::Value for the delta types
    assert!(
        !result.contains("ContentBlockDelta(serde_json::Value)"),
        "ContentBlockDelta should not contain serde_json::Value"
    );

    // Verify variants - check for different possible formats
    assert!(
        result.contains("TextDelta"),
        "Should contain TextDelta variant"
    );
    assert!(
        result.contains("InputJsonDelta"),
        "Should contain InputJsonDelta variant"
    );
}

#[test]
fn test_array_of_discriminated_unions() {
    // Test that arrays of discriminated unions generate properly typed Vec<T>
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ToolList": {
                    "type": "array",
                    "items": {"$ref": "#/components/schemas/Tool"}
                },
                "Tool": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/FunctionTool"},
                        {"$ref": "#/components/schemas/RetrievalTool"}
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    }
                },
                "FunctionTool": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "function"},
                        "name": {"type": "string"},
                        "description": {"type": "string"}
                    },
                    "required": ["type", "name"]
                },
                "RetrievalTool": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "retrieval"},
                        "query": {"type": "string"}
                    },
                    "required": ["type", "query"]
                }
            }
        }
    });

    let result = test_generation("array_union_test", spec).expect("Generation failed");

    // Should have Tool enum
    assert!(
        result.contains("pub enum Tool"),
        "Tool should be generated as an enum"
    );

    // ToolList should be Vec<Tool> type alias or similar
    assert!(
        result.contains("pub type ToolList = Vec<Tool>"),
        "ToolList should be Vec<Tool>, not Vec<serde_json::Value>"
    );
}

#[test]
fn test_reference_resolution_in_complex_schemas() {
    // Test that schema references are properly resolved instead of becoming serde_json::Value
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Message": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "usage": {"$ref": "#/components/schemas/Usage"}
                    },
                    "required": ["id", "usage"]
                },
                "Usage": {
                    "type": "object",
                    "properties": {
                        "input_tokens": {"type": "integer"},
                        "output_tokens": {"type": "integer"}
                    },
                    "required": ["input_tokens", "output_tokens"]
                }
            }
        }
    });

    let result = test_generation("ref_resolution_test", spec).expect("Generation failed");

    // Should have Usage struct
    assert!(
        result.contains("pub struct Usage"),
        "Usage should be generated as a struct"
    );

    // Message should reference Usage, not serde_json::Value
    assert!(
        result.contains("pub usage: Usage"),
        "Message should have usage field of type Usage"
    );
    assert!(
        !result.contains("pub usage: serde_json::Value"),
        "Message should not contain serde_json::Value for usage field"
    );
}

#[test]
fn test_nested_allof_composition() {
    // Test AllOf composition that should flatten properties instead of using serde_json::Value
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "TextMessage": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseMessage"},
                        {
                            "type": "object",
                            "properties": {
                                "content": {"type": "string"}
                            },
                            "required": ["content"]
                        }
                    ]
                },
                "BaseMessage": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "timestamp": {"type": "string"}
                    },
                    "required": ["id", "timestamp"]
                }
            }
        }
    });

    let result = test_generation("allof_test", spec).expect("Generation failed");

    // Should have TextMessage with all properties flattened
    assert!(result.contains("pub struct TextMessage"));
    assert!(result.contains("pub id: String"));
    assert!(result.contains("pub timestamp: String"));
    assert!(result.contains("pub content: String"));
}

#[test]
fn test_allof_with_redundant_type_object_sibling() {
    // Test AllOf composition that should flatten properties instead of using serde_json::Value
    // even with type: object.
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Widget": {
                    "type": "object",
                    "allOf": [
                        {"$ref": "#/components/schemas/WidgetBase"},
                        {"$ref": "#/components/schemas/WidgetExtra"}
                    ]
                },
                "WidgetBase": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"}
                    }
                },
                "WidgetExtra": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"}
                    }
                }
            }
        }
    });

    let result =
        test_generation("allof_type_object_sibling_test", spec).expect("Generation failed");

    assert!(result.contains("pub struct Widget"));
    // Check different possible formats of string.
    assert!(result.contains("pub id: Option<String>") || result.contains("pub id: String"));
    assert!(result.contains("pub name: Option<String>") || result.contains("pub name: String"));
    assert!(!result.contains("pub type Widget = serde_json::Value"));
}

#[test]
fn test_object_with_additional_properties() {
    // Test that objects with additionalProperties correctly use BTreeMap<String, serde_json::Value>
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "DynamicObject": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"}
                    },
                    "required": ["name"],
                    "additionalProperties": true
                }
            }
        }
    });

    let result = test_generation("additional_props_test", spec).expect("Generation failed");

    // Verify additionalProperties generates BTreeMap<String, serde_json::Value>
    assert!(result.contains("pub struct DynamicObject"));
    assert!(
        result.contains("#[serde(flatten)]")
            && result.contains("BTreeMap<String, serde_json::Value>"),
        "DynamicObject should have flattened HashMap for additional properties"
    );
}

#[test]
fn test_error_handling_discriminated_unions() {
    // Test error types that are currently falling back to serde_json::Value
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "StreamError": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/ApiError"},
                        {"$ref": "#/components/schemas/ValidationError"}
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    }
                },
                "ApiError": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "api_error"},
                        "message": {"type": "string"},
                        "code": {"type": "integer"}
                    },
                    "required": ["type", "message", "code"]
                },
                "ValidationError": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "validation_error"},
                        "field": {"type": "string"},
                        "reason": {"type": "string"}
                    },
                    "required": ["type", "field", "reason"]
                }
            }
        }
    });

    let result = test_generation("error_union_test", spec).expect("Generation failed");

    // Verify error unions generate proper enum types
    assert!(
        result.contains("pub enum StreamError"),
        "StreamError should be generated as an enum"
    );

    assert!(
        !result.contains("StreamError(serde_json::Value)"),
        "StreamError should not contain serde_json::Value"
    );

    assert!(
        result.contains("ApiError"),
        "Should contain ApiError variant"
    );
    assert!(
        result.contains("ValidationError"),
        "Should contain ValidationError variant"
    );
}

// Note: Additional test cases for comprehensive serde_json::Value reduction
// will be implemented as we enhance the generator with proper discriminated
// union support, reference resolution, and inline type generation

#[test]
fn test_count_serde_json_value_occurrences() {
    // Meta-test to track reduction in serde_json::Value usage over time
    use std::fs;
    use std::path::Path;

    let generated_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("anthropic/src/generated/types.rs");

    if generated_path.exists() {
        let content = fs::read_to_string(&generated_path).unwrap();
        let count = content.matches("serde_json::Value").count();

        // Current baseline: 50 occurrences
        // Goal: Reduce to ~18-20 (only truly dynamic fields)
        println!("Current serde_json::Value occurrences: {count}");

        // This will fail once we implement the fixes, reminding us to update the baseline
        assert!(
            count <= 50,
            "serde_json::Value usage has increased! Current: {count}, Expected: <= 50"
        );

        // Future goal after implementing discriminated unions
        // assert!(count <= 20, "Still too many serde_json::Value occurrences: {}. Target: <= 20", count);
    }
}
