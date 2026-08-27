#![cfg(feature = "test-helpers")]

//! Test that structs used in both tagged enums and standalone arrays
//! serialize correctly in both contexts.
//!
//! Reproduces the Anthropic `system` field bug where `RequestTextBlock` used to
//! have its `type` field stripped for use in `InputContentBlock`, leaving
//! standalone `RequestTextBlock` values unable to satisfy their own schema.

use openapi_to_rust::test_helpers::*;
use serde_json::json;

/// Models the Anthropic API pattern:
/// - `InputContentBlock` is a discriminated union (oneOf + discriminator) containing `RequestTextBlock`
/// - `CreateMessageParams.system` is `anyOf[string, array[RequestTextBlock]]` (standalone array)
///
/// The generator must ensure `RequestTextBlock` serializes with `type: "text"` in both contexts.
#[test]
fn test_discriminator_stripped_struct_in_standalone_array() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "RequestTextBlock": {
                    "type": "object",
                    "properties": {
                        "type": { "const": "text" },
                        "text": { "type": "string" },
                        "cache_control": {
                            "type": "object",
                            "properties": {
                                "type": { "const": "ephemeral" },
                                "ttl": { "type": "string" }
                            }
                        }
                    },
                    "required": ["type", "text"]
                },
                "RequestImageBlock": {
                    "type": "object",
                    "properties": {
                        "type": { "const": "image" },
                        "source": { "type": "string" }
                    },
                    "required": ["type", "source"]
                },
                "InputContentBlock": {
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
                },
                "CreateMessageParams": {
                    "type": "object",
                    "properties": {
                        "messages": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/InputContentBlock" }
                        },
                        "system": {
                            "anyOf": [
                                { "type": "string" },
                                {
                                    "type": "array",
                                    "items": { "$ref": "#/components/schemas/RequestTextBlock" }
                                }
                            ]
                        }
                    },
                    "required": ["messages"]
                }
            }
        }
    });

    let result =
        test_generation("discriminator_array_standalone", spec).expect("Generation failed");

    // The standalone struct remains schema-faithful, including its required
    // discriminator field, so the array can safely use it directly.
    assert!(
        result.contains("pub r#type: RequestTextBlockType")
            || result.contains("pub r#type: serde_json::Value"),
        "RequestTextBlock must retain its required discriminator field.\n\
         Generated:\n{result}"
    );
    assert!(
        result.contains("pub type RequestTextBlockArray = Vec<RequestTextBlock>"),
        "A schema-faithful RequestTextBlock should be reusable directly in arrays.\n\
         Generated:\n{result}"
    );
    assert!(
        result.contains("match discriminator.as_str()"),
        "InputContentBlock must deserialize by inspecting its discriminator.\n\
         Generated:\n{result}"
    );
}

/// When a required field has a default value in the spec but its type is a
/// discriminated union (which doesn't derive Default), the generator should
/// make it `Option<T>` instead of bare `T` with `#[serde(default)]`.
#[test]
fn test_default_field_with_discriminated_union_type() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "CallerType": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/DirectCaller" },
                        { "$ref": "#/components/schemas/ServerCaller" }
                    ],
                    "discriminator": {
                        "propertyName": "type",
                        "mapping": {
                            "direct": "#/components/schemas/DirectCaller",
                            "server": "#/components/schemas/ServerCaller"
                        }
                    }
                },
                "DirectCaller": {
                    "type": "object",
                    "properties": {
                        "type": { "const": "direct" }
                    },
                    "required": ["type"]
                },
                "ServerCaller": {
                    "type": "object",
                    "properties": {
                        "type": { "const": "server" },
                        "version": { "type": "string" }
                    },
                    "required": ["type"]
                },
                "ToolResult": {
                    "type": "object",
                    "properties": {
                        "content": { "type": "string" },
                        "caller": {
                            "default": { "type": "direct" },
                            "oneOf": [
                                { "$ref": "#/components/schemas/DirectCaller" },
                                { "$ref": "#/components/schemas/ServerCaller" }
                            ],
                            "discriminator": {
                                "propertyName": "type",
                                "mapping": {
                                    "direct": "#/components/schemas/DirectCaller",
                                    "server": "#/components/schemas/ServerCaller"
                                }
                            }
                        }
                    },
                    "required": ["content", "caller"]
                }
            }
        }
    });

    let result =
        test_generation("default_discriminated_union_field", spec).expect("Generation failed");

    // The `caller` field should be Option<CallerType>, not bare CallerType with #[serde(default)]
    // because CallerType is a discriminated union that doesn't implement Default.
    assert!(
        !result.contains("#[serde(default)]"),
        "Should NOT have #[serde(default)] on a discriminated union field.\n\
         Generated:\n{result}"
    );
    // The inline discriminated union gets named ToolResultCaller (context-aware).
    assert!(
        result.contains("Option<ToolResultCaller>") || result.contains("Option<CallerType>"),
        "Discriminated union field with default should be Option<T>.\n\
         Generated:\n{result}"
    );
}
