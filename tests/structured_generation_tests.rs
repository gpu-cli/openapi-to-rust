#![cfg(feature = "test-helpers")]

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn test_content_union_structured() {
    let spec = minimal_spec(json!({
        "InputMessage": {
            "type": "object",
            "properties": {
                "role": {"type": "string"},
                "content": {
                    "anyOf": [
                        {"type": "string"},
                        {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/ContentBlock"}
                        }
                    ]
                }
            },
            "required": ["role", "content"]
        },
        "ContentBlock": {
            "type": "object",
            "properties": {
                "type": {"type": "string"},
                "text": {"type": "string"}
            },
            "required": ["type", "text"]
        }
    }));

    let result = test_generation("content_union_structured", spec).expect("Generation failed");

    // Verify structs and enums are generated
    assert!(result.contains("pub struct InputMessage"));
    assert!(result.contains("pub struct ContentBlock"));
    assert!(result.contains("pub enum InputMessageContent"));

    // Verify the enum variants
    assert!(result.contains("String") || result.contains("InputMessageContentString"));
    assert!(result.contains("ContentBlockArray") || result.contains("Array"));

    // Verify no underscores in type names
    assert!(!result.contains("Input_Message"));
    assert!(!result.contains("Content_Block"));
}

#[test]
fn test_underscore_properties_structured() {
    let spec = minimal_spec(json!({
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
                        {"type": "string"}
                    ]
                }
            }
        }
    }));

    let result = test_generation("underscore_props_structured", spec).expect("Generation failed");

    // Verify the struct is generated
    assert!(result.contains("pub struct ConfigSchema"));

    // Optional nullable properties preserve missing versus explicit null.
    assert!(
        result.contains("ConfigSchemaAllowedTools")
            || result.contains("pub allowed_tools: Option<Option<Vec<String>>>")
    );
    assert!(
        result.contains("ConfigSchemaCacheControl")
            || result.contains("pub cache_control: Option<Option<String>>")
    );

    // Verify no underscores in generated type names
    assert!(!result.contains("Config_Schema"));
    assert!(!result.contains("Allowed_Tools"));
    assert!(!result.contains("Cache_Control"));
}

#[test]
fn test_discriminated_union_structured() {
    let spec = minimal_spec(json!({
        "Response": {
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "type": {"const": "success"},
                        "data": {"type": "string"}
                    }
                },
                {
                    "type": "object",
                    "properties": {
                        "type": {"const": "error"},
                        "message": {"type": "string"}
                    }
                }
            ],
            "discriminator": {
                "propertyName": "type"
            }
        }
    }));

    let result =
        test_generation("discriminated_union_structured", spec).expect("Generation failed");

    // Verify Response enum is generated
    assert!(result.contains("pub enum Response"));

    // Verify enum has proper variants
    assert!(result.contains("Success") || result.contains("ResponseSuccess"));
    assert!(result.contains("Error") || result.contains("ResponseError"));
}

#[test]
fn test_complex_nested_schema() {
    let spec = minimal_spec(json!({
        "BetaListResponse_MessageBatch": {
            "type": "object",
            "properties": {
                "last_id": {
                    "anyOf": [
                        {"type": "null"},
                        {"type": "string"}
                    ]
                },
                "data": {
                    "type": "array",
                    "items": {"$ref": "#/components/schemas/MessageBatch"}
                }
            }
        },
        "MessageBatch": {
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "status": {"type": "string", "enum": ["pending", "completed", "failed"]}
            }
        }
    }));

    let result = test_generation("complex_nested", spec).expect("Generation failed");

    // Verify structs are generated with proper names (underscores converted to camelCase)
    assert!(result.contains("pub struct BetaListResponseMessageBatch"));
    assert!(result.contains("pub struct MessageBatch"));

    // Verify status enum is generated
    assert!(result.contains("pub enum MessageBatchStatus"));

    // The nullable pattern avoids a synthetic union type, while the two
    // Options retain field presence and explicit JSON null independently.
    assert!(result.contains("pub last_id: Option<Option<String>>"));

    // Check union types don't have underscores
    assert!(!result.contains("Beta_List_Response"));
    assert!(!result.contains("Message_Batch"));
    assert!(!result.contains("Last_Id"));
}

#[test]
fn test_nullable_fields_structured() {
    let spec = minimal_spec(json!({
        "User": {
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "name": {"type": "string"},
                "email": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "null"}
                    ]
                },
                "metadata": {
                    "anyOf": [
                        {"type": "object"},
                        {"type": "null"}
                    ]
                }
            },
            "required": ["id", "name"]
        }
    }));

    let result = test_generation("nullable_fields", spec).expect("Generation failed");

    // Verify User struct is generated
    assert!(result.contains("pub struct User"));

    // Verify nullable field types are handled properly
    assert!(result.contains("email: Option<Option<String>>"));
    assert!(result.contains("metadata: Option<Option<serde_json::Value>>"));
}
