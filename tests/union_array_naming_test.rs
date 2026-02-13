//! Test for improved union array naming (fixing UnionArray1 -> contextual names)

use openapi_generator::test_helpers::*;
use serde_json::json;

#[test]
fn test_union_array_context_aware_naming() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "RequestToolResultBlock": {
                    "type": "object",
                    "properties": {
                        "tool_use_id": {
                            "type": "string",
                            "pattern": "^[a-zA-Z0-9_-]+$"
                        },
                        "content": {
                            "anyOf": [
                                { "type": "string" },
                                {
                                    "type": "array",
                                    "items": {
                                        "oneOf": [
                                            { "$ref": "#/components/schemas/RequestTextBlock" },
                                            { "$ref": "#/components/schemas/RequestImageBlock" }
                                        ],
                                        "discriminator": {
                                            "propertyName": "type",
                                            "mapping": {
                                                "text": "#/components/schemas/RequestTextBlock",
                                                "image": "#/components/schemas/RequestImageBlock"
                                            }
                                        }
                                    }
                                }
                            ]
                        }
                    },
                    "required": ["tool_use_id"]
                },
                "RequestTextBlock": {
                    "type": "object",
                    "properties": {
                        "type": { "const": "text" },
                        "text": { "type": "string" }
                    },
                    "required": ["type", "text"]
                },
                "RequestImageBlock": {
                    "type": "object",
                    "properties": {
                        "type": { "const": "image" },
                        "source": {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string" },
                                "data": { "type": "string" }
                            }
                        }
                    },
                    "required": ["type", "source"]
                },
                "MessageContent": {
                    "anyOf": [
                        { "type": "string" },
                        {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "text": { "type": "string" }
                                }
                            }
                        }
                    ]
                }
            }
        }
    });

    let result = test_generation("union_array_naming", spec).expect("Generation failed");

    // Check that UnionArray1 is NOT present
    assert!(
        !result.contains("UnionArray1"),
        "Should not contain generic UnionArray1 names"
    );
    assert!(
        !result.contains("UnionArray2"),
        "Should not contain generic UnionArray2 names"
    );

    // Check for context-aware array naming in RequestToolResultBlock
    assert!(
        result.contains("RequestToolResultBlockContentArray")
            || result.contains("RequestToolResultBlockArray"),
        "Array variant in RequestToolResultBlock should have context-aware name"
    );

    // Check that the enum is properly structured
    assert!(
        result.contains("pub enum RequestToolResultBlockContent"),
        "Should have RequestToolResultBlockContent enum"
    );

    // For MessageContent, should have context-aware array names
    assert!(
        result.contains("MessageContentArray")
            || result.contains("MessageContentStringArray")
            || result.contains("Vec<String>"),
        "MessageContent arrays should have context-aware names"
    );
}

#[test]
fn test_nested_union_array_naming() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "ComplexContent": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "simple": { "type": "string" }
                            }
                        },
                        {
                            "type": "array",
                            "items": {
                                "anyOf": [
                                    { "type": "string" },
                                    { "$ref": "#/components/schemas/NestedItem" }
                                ]
                            }
                        }
                    ]
                },
                "NestedItem": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "value": { "type": "number" }
                    }
                }
            }
        }
    });

    let result = test_generation("nested_union_array", spec).expect("Generation failed");

    // Should not have generic numbered array names
    assert!(
        !result.contains("UnionArray"),
        "Should not contain any UnionArray names"
    );

    // Should have context-aware names
    assert!(
        result.contains("ComplexContentArray") || result.contains("ComplexContentItemArray"),
        "Nested array should have context-aware name"
    );
}

#[test]
fn test_multiple_array_variants_naming() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "MultiArrayContent": {
                    "anyOf": [
                        { "type": "string" },
                        {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        {
                            "type": "array",
                            "items": { "type": "integer" }
                        },
                        {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/Item" }
                        }
                    ]
                },
                "Item": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                }
            }
        }
    });

    let result = test_generation("multi_array_variants", spec).expect("Generation failed");

    // Should not have numbered array names
    for i in 0..10 {
        assert!(
            !result.contains(&format!("UnionArray{i}")),
            "Should not contain UnionArray{i} name"
        );
    }

    // Arrays should either be inlined as Vec<T> or have descriptive names
    assert!(
        result.contains("Vec<String>") || result.contains("MultiArrayContentStringArray"),
        "String array should be properly typed"
    );

    assert!(
        result.contains("Vec<i32>")
            || result.contains("Vec<i64>")
            || result.contains("MultiArrayContentIntegerArray"),
        "Integer array should be properly typed"
    );

    assert!(
        result.contains("Vec<Item>")
            || result.contains("ItemArray")
            || result.contains("MultiArrayContentItemArray"),
        "Item array should use referenced type name"
    );
}
