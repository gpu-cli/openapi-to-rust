use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn test_allof_wrapper_in_oneof() {
    // This tests the issue where oneOf contains both direct references and allOf wrappers
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "AllOf Wrapper Test",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "TextBlock": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["text"]
                        },
                        "text": {
                            "type": "string"
                        }
                    },
                    "required": ["type", "text"]
                },
                "ImageBlock": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["image"]
                        },
                        "url": {
                            "type": "string"
                        }
                    },
                    "required": ["type", "url"]
                },
                "DocumentBlock": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["document"]
                        },
                        "content": {
                            "type": "string"
                        }
                    },
                    "required": ["type", "content"]
                },
                "ContentBlock": {
                    "oneOf": [
                        {
                            "$ref": "#/components/schemas/TextBlock"
                        },
                        {
                            "allOf": [
                                {"$ref": "#/components/schemas/ImageBlock"}
                            ],
                            "description": "Image content"
                        },
                        {
                            "allOf": [
                                {"$ref": "#/components/schemas/DocumentBlock"}
                            ]
                        }
                    ],
                    "discriminator": {
                        "propertyName": "type",
                        "mapping": {
                            "text": "#/components/schemas/TextBlock",
                            "image": "#/components/schemas/ImageBlock",
                            "document": "#/components/schemas/DocumentBlock"
                        }
                    }
                }
            }
        }
    });

    let result = test_generation("allof_wrapper_in_oneof", spec).expect("Generation failed");

    // Verify all block types are generated
    assert!(result.contains("pub struct TextBlock"));
    assert!(result.contains("pub struct ImageBlock"));
    assert!(result.contains("pub struct DocumentBlock"));

    // Verify ContentBlock is an enum
    assert!(result.contains("pub enum ContentBlock"));

    // Check for proper enum variants - just check the types exist
    assert!(result.contains("TextBlock"));
    assert!(result.contains("ImageBlock"));
    assert!(result.contains("DocumentBlock"));
}

#[test]
fn test_underscore_in_type_names() {
    // This tests that underscores in schema names are properly converted to PascalCase
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Underscore Type Names Test",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Beta_List_Response_Message_Batch": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "last_id": {"$ref": "#/components/schemas/Beta_List_Response_Message_Batch_Last_Id"}
                    },
                    "required": ["id"]
                },
                "Beta_List_Response_Message_Batch_Last_Id": {
                    "type": "string",
                    "description": "Last ID type"
                },
                "Beta_Tool_20241022_Cache_Control": {
                    "type": "string",
                    "enum": ["ephemeral"]
                },
                "Response_Stream_Event": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/Response_Created_Event"},
                        {"$ref": "#/components/schemas/Response_Completed_Event"}
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    }
                },
                "Response_Created_Event": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["created"]},
                        "id": {"type": "string"}
                    },
                    "required": ["type", "id"]
                },
                "Response_Completed_Event": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["completed"]},
                        "id": {"type": "string"}
                    },
                    "required": ["type", "id"]
                }
            }
        }
    });

    let result = test_generation("underscore_type_names", spec).expect("Generation failed");

    // Verify type names are converted from underscores to PascalCase
    assert!(result.contains("pub struct BetaListResponseMessageBatch"));
    assert!(result.contains("pub type BetaListResponseMessageBatchLastId = String"));
    assert!(result.contains("pub enum BetaTool20241022CacheControl"));
    assert!(result.contains("pub enum ResponseStreamEvent"));
    assert!(result.contains("pub struct ResponseCreatedEvent"));
    assert!(result.contains("pub struct ResponseCompletedEvent"));
}

#[test]
fn test_property_name_with_underscores_creates_types() {
    // Tests that property names with underscores in anyOf unions get proper type names
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Property Names with Underscores Test",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "ConfigObject": {
                    "type": "object",
                    "properties": {
                        "display_settings": {
                            "anyOf": [
                                {"type": "string"},
                                {
                                    "type": "object",
                                    "properties": {
                                        "width": {"type": "integer"},
                                        "height": {"type": "integer"}
                                    },
                                    "required": ["width", "height"]
                                }
                            ]
                        },
                        "cache_control": {
                            "anyOf": [
                                {"type": "string", "enum": ["ephemeral"]},
                                {"type": "null"}
                            ]
                        }
                    },
                    "required": ["display_settings"]
                }
            }
        }
    });

    let result = test_generation("property_underscore_types", spec).expect("Generation failed");

    // Verify ConfigObject struct is generated
    assert!(result.contains("pub struct ConfigObject"));

    // Verify display_settings field uses proper enum type
    assert!(result.contains("pub display_settings: ConfigObjectDisplaySettings"));

    // Verify cache_control is optional enum type
    assert!(result.contains("pub cache_control: Option<ConfigObjectCacheControl>"));
}

#[test]
fn test_nested_allof_composition() {
    // Tests deeply nested allOf structures
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Nested AllOf Test",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "BaseMessage": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"}
                    },
                    "required": ["id"]
                },
                "TypedMessage": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseMessage"},
                        {
                            "type": "object",
                            "properties": {
                                "msg_type": {"type": "string"}
                            },
                            "required": ["msg_type"]
                        }
                    ]
                },
                "TextMessage": {
                    "allOf": [
                        {"$ref": "#/components/schemas/TypedMessage"},
                        {
                            "type": "object",
                            "properties": {
                                "text": {"type": "string"}
                            },
                            "required": ["text"]
                        }
                    ]
                }
            }
        }
    });

    let result = test_generation("nested_allof_composition", spec).expect("Generation failed");

    // All three structs should be generated
    assert!(result.contains("pub struct BaseMessage"));
    assert!(result.contains("pub struct TypedMessage"));
    assert!(result.contains("pub struct TextMessage"));

    // TextMessage should have all flattened fields
    let text_msg_start = result
        .find("pub struct TextMessage")
        .expect("TextMessage should exist");
    let text_msg_end = result[text_msg_start..].find("}").unwrap() + text_msg_start;
    let text_msg_section = &result[text_msg_start..=text_msg_end];

    assert!(text_msg_section.contains("id") && text_msg_section.contains(": String"));
    assert!(text_msg_section.contains("msg_type") && text_msg_section.contains(": String"));
    assert!(text_msg_section.contains("text") && text_msg_section.contains(": String"));
}

#[test]
fn test_discriminator_without_explicit_mapping() {
    // Tests that discriminator without explicit mapping still works with allOf
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Discriminator Without Mapping Test",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Animal": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/Cat"},
                        {
                            "allOf": [{"$ref": "#/components/schemas/Dog"}]
                        }
                    ],
                    "discriminator": {
                        "propertyName": "species"
                    }
                },
                "Cat": {
                    "type": "object",
                    "properties": {
                        "species": {"type": "string", "enum": ["cat"]},
                        "meow": {"type": "boolean"}
                    },
                    "required": ["species", "meow"]
                },
                "Dog": {
                    "type": "object",
                    "properties": {
                        "species": {"type": "string", "enum": ["dog"]},
                        "bark": {"type": "boolean"}
                    },
                    "required": ["species", "bark"]
                }
            }
        }
    });

    let result = test_generation("discriminator_no_mapping", spec).expect("Generation failed");

    // Verify structs are generated
    assert!(result.contains("pub struct Cat"));
    assert!(result.contains("pub struct Dog"));
    assert!(result.contains("pub enum Animal"));

    // Verify enum has proper variants
    assert!(result.contains("Cat(Cat)") || result.contains("Cat {"));
    assert!(result.contains("Dog(Dog)") || result.contains("Dog {"));
}

#[test]
fn test_multiple_types_same_discriminator_value() {
    // Tests that we handle duplicate discriminator values correctly
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Duplicate Discriminator Test",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "SimpleText": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["text"]},
                        "value": {"type": "string"}
                    },
                    "required": ["type", "value"]
                },
                "RichText": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["text"]},
                        "html": {"type": "string"},
                        "markdown": {"type": "string"}
                    },
                    "required": ["type", "html"]
                },
                "Content": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/SimpleText"},
                        {"$ref": "#/components/schemas/RichText"}
                    ],
                    "discriminator": {
                        "propertyName": "type",
                        "mapping": {
                            "text": "#/components/schemas/SimpleText",
                            "rich_text": "#/components/schemas/RichText"
                        }
                    }
                }
            }
        }
    });

    let result =
        test_generation("duplicate_discriminator_values", spec).expect("Generation failed");

    // Verify all components are generated
    assert!(result.contains("pub struct SimpleText"));
    assert!(result.contains("pub struct RichText"));
    assert!(result.contains("pub enum Content"));

    // Both types have the same discriminator value "text"
    // The generator should handle this gracefully
}

#[test]
fn test_inline_and_ref_mixed_in_oneof() {
    // Tests inline schemas mixed with references in oneOf, some wrapped in allOf
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Mixed Inline and Ref Test",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "NamedError": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["error"]},
                        "code": {"type": "string"},
                        "message": {"type": "string"}
                    },
                    "required": ["type", "code", "message"]
                },
                "Response": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "type": {"type": "string", "enum": ["success"]},
                                "data": {"type": "object"}
                            },
                            "required": ["type", "data"]
                        },
                        {
                            "allOf": [{"$ref": "#/components/schemas/NamedError"}]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": {"type": "string", "enum": ["redirect"]},
                                "url": {"type": "string"}
                            },
                            "required": ["type", "url"]
                        }
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    }
                }
            }
        }
    });

    let result = test_generation("mixed_inline_ref_oneof", spec).expect("Generation failed");

    // Verify NamedError struct is generated
    assert!(result.contains("pub struct NamedError"));

    // Verify Response enum is generated
    assert!(result.contains("pub enum Response"));

    // Check that Response has proper variants
    assert!(result.contains("NamedError"));
    assert!(result.contains("Success") || result.contains("ResponseSuccess"));
    assert!(result.contains("Redirect") || result.contains("ResponseRedirect"));
}
