//! Tests for enum improvements including:
//! - Inline enum extraction from properties
//! - Const + enum support
//! - Extensible enums (anyOf with const values)
//! - OneOf discriminated unions in properties
//! - Array union item naming

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn test_inline_enum_extraction_from_property() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Message": {
                    "type": "object",
                    "properties": {
                        "role": {
                            "type": "string",
                            "enum": ["user", "assistant", "system"],
                            "description": "The role of the message author"
                        },
                        "content": {
                            "type": "string"
                        }
                    },
                    "required": ["role", "content"]
                }
            }
        }
    });

    let result = test_generation("inline_enum_extraction", spec).expect("Generation failed");

    // Verify the generated code contains expected types
    assert!(result.contains("pub struct Message"));
    assert!(result.contains("pub enum MessageRole"));
    assert!(result.contains("User"));
    assert!(result.contains("Assistant"));
    assert!(result.contains("System"));
}

#[test]
fn test_property_with_const_and_enum() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "AssistantMessage": {
                    "type": "object",
                    "properties": {
                        "role": {
                            "type": "string",
                            "const": "assistant",
                            "enum": ["assistant"],
                            "description": "Always 'assistant' for this message type"
                        },
                        "content": {
                            "type": "string"
                        }
                    },
                    "required": ["role", "content"]
                }
            }
        }
    });

    let result = test_generation("const_and_enum", spec).expect("Generation failed");

    // Verify the const field is handled properly
    assert!(result.contains("pub struct AssistantMessage"));
    assert!(result.contains("pub enum AssistantMessageRole"));
    assert!(result.contains("Assistant"));
}

#[test]
fn test_extensible_enum_anyof_const_values() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Model": {
                    "title": "Model",
                    "anyOf": [
                        {
                            "type": "string",
                            "const": "claude-3-opus"
                        },
                        {
                            "type": "string",
                            "const": "claude-3-sonnet"
                        },
                        {
                            "type": "string",
                            "const": "claude-3-haiku"
                        },
                        {
                            "type": "string",
                            "description": "Custom model identifier"
                        }
                    ],
                    "description": "Available model identifiers"
                },
                "CreateMessageParams": {
                    "type": "object",
                    "properties": {
                        "model": {
                            "$ref": "#/components/schemas/Model"
                        },
                        "content": {
                            "type": "string"
                        }
                    },
                    "required": ["model", "content"]
                }
            }
        }
    });

    let result = test_generation("extensible_enum", spec).expect("Generation failed");

    // Verify the extensible enum is generated correctly
    assert!(result.contains("pub enum Model"));
    assert!(result.contains("Claude3Opus"));
    assert!(result.contains("Claude3Sonnet"));
    assert!(result.contains("Claude3Haiku"));
    assert!(
        result.contains("Other") || result.contains("Custom"),
        "Should contain Other/Custom variant for extensible enum"
    );
    assert!(result.contains("pub struct CreateMessageParams"));
}

#[test]
fn test_oneof_discriminated_union_in_property() {
    let spec = json!({
        "openapi": "3.1.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "components": {
                "schemas": {
                    "ImageBlock": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "const": "image"
                            },
                            "source": {
                                "oneOf": [
                                    {"$ref": "#/components/schemas/Base64ImageSource"},
                                    {"$ref": "#/components/schemas/URLImageSource"}
                                ],
                                "discriminator": {
                                    "propertyName": "type",
                                    "mapping": {
                                        "base64": "#/components/schemas/Base64ImageSource",
                                        "url": "#/components/schemas/URLImageSource"
                                    }
                                }
                            }
                        },
                        "required": ["type", "source"]
                    },
                    "Base64ImageSource": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "const": "base64"
                            },
                            "data": {
                                "type": "string"
                            }
                        },
                        "required": ["type", "data"]
                    },
                    "URLImageSource": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "const": "url"
                            },
                            "url": {
                                "type": "string"
                            }
                        },
                        "required": ["type", "url"]
                    }
                }
            }
    });

    let result = test_generation("oneof_in_property", spec).expect("Generation failed");

    // Verify the discriminated union in property is handled
    assert!(result.contains("pub struct ImageBlock"));
    assert!(result.contains("pub enum ImageBlockSource"));
    assert!(result.contains("Base64ImageSource"));
    assert!(
        result.contains("URLImageSource") || result.contains("UrlImageSource"),
        "Should contain URL/Url ImageSource struct"
    );
}

#[test]
fn test_array_with_union_items() {
    let spec = json!({
        "openapi": "3.1.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "components": {
                "schemas": {
                    "ToolsRequest": {
                        "type": "object",
                        "properties": {
                            "tools": {
                                "type": "array",
                                "items": {
                                    "oneOf": [
                                        {"$ref": "#/components/schemas/TextTool"},
                                        {"$ref": "#/components/schemas/CodeTool"}
                                    ],
                                    "discriminator": {
                                        "propertyName": "type"
                                    }
                                }
                            }
                        },
                        "required": ["tools"]
                    },
                    "TextTool": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "const": "text"
                            },
                            "name": {
                                "type": "string"
                            }
                        },
                        "required": ["type", "name"]
                    },
                    "CodeTool": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "const": "code"
                            },
                            "language": {
                                "type": "string"
                            }
                        },
                        "required": ["type", "language"]
                    }
                }
            }
    });

    let result = test_generation("array_union_items", spec).expect("Generation failed");

    // Verify array with union items is handled
    assert!(result.contains("pub struct ToolsRequest"));
    assert!(result.contains("pub enum ToolsRequestToolsItem"));
    assert!(result.contains("Text") && result.contains("TextTool"));
    assert!(result.contains("Code") && result.contains("CodeTool"));
    assert!(result.contains("pub struct TextTool"));
    assert!(result.contains("pub struct CodeTool"));
}

#[test]
fn test_multiple_properties_with_enums() {
    // Test that multiple properties with enum values in the same schema get unique enum names
    let spec = json!({
        "openapi": "3.1.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "components": {
                "schemas": {
                    "Task": {
                        "type": "object",
                        "properties": {
                            "status": {
                                "type": "string",
                                "enum": ["pending", "running", "completed", "failed"],
                                "description": "Task status"
                            },
                            "priority": {
                                "type": "string",
                                "enum": ["low", "medium", "high", "critical"],
                                "description": "Task priority"
                            },
                            "type": {
                                "type": "string",
                                "enum": ["build", "test", "deploy"],
                                "description": "Task type"
                            }
                        },
                        "required": ["status", "priority", "type"]
                    }
                }
            }
    });

    let result = test_generation("multiple_enum_properties", spec).expect("Generation failed");

    // Verify multiple enum properties generate unique enum names
    assert!(result.contains("pub struct Task"));
    assert!(result.contains("pub enum TaskStatus"));
    assert!(result.contains("pub enum TaskPriority"));
    assert!(result.contains("pub enum TaskType"));
    assert!(result.contains("Pending"));
    assert!(result.contains("Running"));
    assert!(result.contains("Low"));
    assert!(result.contains("High"));
    assert!(result.contains("Build"));
    assert!(result.contains("Deploy"));
}

#[test]
fn test_extensible_enum_serialization() {
    // Test that extensible enums serialize correctly to their string values
    let spec = json!({
        "openapi": "3.1.0",
            "info": {
                "title": "Test API",
                "version": "1.0.0"
            },
            "components": {
                "schemas": {
                    "Model": {
                        "title": "Model",
                        "anyOf": [
                            {
                                "type": "string",
                                "const": "gpt-4"
                            },
                            {
                                "type": "string",
                                "const": "gpt-3.5-turbo"
                            },
                            {
                                "type": "string",
                                "const": "claude-3-opus"
                            },
                            {
                                "type": "string",
                                "description": "Custom model identifier"
                            }
                        ],
                        "description": "Available model identifiers"
                    },
                    "Request": {
                        "type": "object",
                        "properties": {
                            "model": {
                                "$ref": "#/components/schemas/Model"
                            },
                            "prompt": {
                                "type": "string"
                            }
                        },
                        "required": ["model", "prompt"]
                    }
                }
            }
    });

    let result = test_generation("extensible_enum_serialization", spec).expect("Generation failed");

    // Check that Model enum has custom Serialize implementation
    assert!(
        result.contains("impl serde::Serialize for Model"),
        "Model enum should have custom Serialize implementation"
    );
    assert!(
        result.contains("impl<'de> serde::Deserialize<'de> for Model"),
        "Model enum should have custom Deserialize implementation"
    );

    // Verify it's not using untagged enum
    assert!(
        !result.contains("#[serde(untagged)]"),
        "Model enum should not use untagged serde attribute"
    );

    // Verify the enum variants
    assert!(result.contains("Gpt4"));
    assert!(result.contains("Gpt35Turbo"));
    assert!(result.contains("Claude3Opus"));
    assert!(
        result.contains("Other") || result.contains("Custom"),
        "Should contain Other/Custom variant for extensible enum"
    );
}
