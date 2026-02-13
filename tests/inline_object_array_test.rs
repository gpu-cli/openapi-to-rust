//! Tests for inline objects in arrays
//!
//! These tests ensure that inline objects in arrays are properly typed
//! instead of falling back to serde_json::Value

use openapi_generator::test_helpers::*;
use serde_json::json;

#[test]
fn test_inline_object_in_array_simple() {
    // Simple test case with inline object in array
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ResponseWithItems": {
                    "type": "object",
                    "properties": {
                        "items": {
                            "type": "array",
                            "description": "Array with inline object items",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string"},
                                    "name": {"type": "string"},
                                    "value": {"type": "integer"}
                                },
                                "required": ["id", "name"]
                            }
                        }
                    },
                    "required": ["items"]
                }
            }
        }
    });

    let result = test_generation("inline_object_in_array_simple", spec).expect("Generation failed");

    // Assert that an item type was generated
    assert!(
        result.contains("pub struct ResponseWithItemsItemsItem"),
        "Should generate a struct for inline object in array"
    );

    // Assert that the array uses the generated type
    assert!(
        result.contains("pub items: Vec<ResponseWithItemsItemsItem>"),
        "Array should use the generated item type"
    );

    // Assert that serde_json::Value is NOT used
    assert!(
        !result.contains("pub items: Vec<serde_json::Value>"),
        "Array should NOT fall back to serde_json::Value"
    );
}

#[test]
fn test_chat_completion_stream_response_choices() {
    // Test case from OpenAI's chat completions API
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "CreateChatCompletionStreamResponse": {
                    "description": "Represents a streamed chunk of a chat completion response",
                    "type": "object",
                    "properties": {
                        "id": {
                            "description": "A unique identifier for the chat completion",
                            "type": "string"
                        },
                        "object": {
                            "description": "The object type, which is always `chat.completion.chunk`",
                            "type": "string",
                            "enum": ["chat.completion.chunk"]
                        },
                        "created": {
                            "description": "The Unix timestamp (in seconds) of when the chat completion was created",
                            "type": "integer"
                        },
                        "model": {
                            "description": "The model to generate the completion",
                            "type": "string"
                        },
                        "choices": {
                            "description": "A list of chat completion choices",
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "index": {
                                        "description": "The index of the choice in the list of choices",
                                        "type": "integer"
                                    },
                                    "delta": {
                                        "$ref": "#/components/schemas/ChatCompletionStreamResponseDelta"
                                    },
                                    "finish_reason": {
                                        "description": "The reason the model stopped generating tokens",
                                        "type": "string",
                                        "nullable": true,
                                        "enum": ["stop", "length", "tool_calls", "content_filter", "function_call"]
                                    },
                                    "logprobs": {
                                        "description": "Log probability information for the choice",
                                        "type": "object",
                                        "nullable": true,
                                        "properties": {
                                            "content": {
                                                "type": "array",
                                                "nullable": true,
                                                "items": {
                                                    "$ref": "#/components/schemas/ChatCompletionTokenLogprob"
                                                }
                                            },
                                            "refusal": {
                                                "type": "array",
                                                "nullable": true,
                                                "items": {
                                                    "$ref": "#/components/schemas/ChatCompletionTokenLogprob"
                                                }
                                            }
                                        },
                                        "required": ["content", "refusal"]
                                    }
                                },
                                "required": ["delta", "finish_reason", "index"]
                            }
                        }
                    },
                    "required": ["choices", "created", "id", "model", "object"]
                },
                "ChatCompletionStreamResponseDelta": {
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "nullable": true
                        },
                        "role": {
                            "type": "string",
                            "enum": ["developer", "system", "user", "assistant", "tool"]
                        }
                    }
                },
                "ChatCompletionTokenLogprob": {
                    "type": "object",
                    "properties": {
                        "token": {"type": "string"},
                        "logprob": {"type": "number"},
                        "bytes": {
                            "type": "array",
                            "nullable": true,
                            "items": {"type": "integer"}
                        }
                    },
                    "required": ["token", "logprob", "bytes"]
                }
            }
        }
    });

    let result = test_generation("content_delta_test", spec).expect("Generation failed");

    // Assert that the choices item type was generated
    assert!(
        result.contains("pub struct CreateChatCompletionStreamResponseChoicesItem")
            || result.contains("pub struct CreateChatCompletionStreamResponseItem"),
        "Should generate a struct for choices array items"
    );

    // Assert that the choices field uses the generated type
    assert!(
        result.contains("pub choices: Vec<CreateChatCompletionStreamResponseChoicesItem>")
            || result.contains("pub choices: Vec<CreateChatCompletionStreamResponseItem>"),
        "Choices field should use the generated item type"
    );

    // Assert that serde_json::Value is NOT used
    assert!(
        !result.contains("pub choices: Vec<serde_json::Value>"),
        "Choices field should NOT fall back to serde_json::Value"
    );
}

#[test]
fn test_nested_inline_objects_in_array() {
    // Test case with nested inline objects
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "NestedResponse": {
                    "type": "object",
                    "properties": {
                        "results": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string"},
                                    "metadata": {
                                        "type": "object",
                                        "properties": {
                                            "tags": {
                                                "type": "array",
                                                "items": {"type": "string"}
                                            },
                                            "attributes": {
                                                "type": "object",
                                                "additionalProperties": {"type": "string"}
                                            }
                                        }
                                    }
                                },
                                "required": ["id"]
                            }
                        }
                    },
                    "required": ["results"]
                }
            }
        }
    });

    let result = test_generation("nested_inline_objects_test", spec).expect("Generation failed");

    // Assert that the results item type was generated
    assert!(
        result.contains("pub struct NestedResponseResultsItem")
            || result.contains("pub struct NestedResponseItem"),
        "Should generate a struct for results array items"
    );

    // Assert proper typing for the nested structure
    assert!(
        result.contains("pub results: Vec<NestedResponseResultsItem>")
            || result.contains("pub results: Vec<NestedResponseItem>"),
        "Results field should use the generated item type"
    );
}

#[test]
fn test_inline_object_property() {
    // Test case for inline objects as properties (like function in ChatCompletionMessageToolCallChunk)
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ChatCompletionMessageToolCallChunk": {
                    "type": "object",
                    "properties": {
                        "function": {
                            "type": "object",
                            "properties": {
                                "arguments": {
                                    "description": "The arguments to call the function with",
                                    "type": "string"
                                },
                                "name": {
                                    "description": "The name of the function to call",
                                    "type": "string"
                                }
                            }
                        },
                        "id": {
                            "description": "The ID of the tool call",
                            "type": "string"
                        },
                        "index": {
                            "type": "integer"
                        }
                    }
                }
            }
        }
    });

    let result = test_generation("inline_object_property", spec).expect("Generation failed");

    // Assert that a type was generated for the inline function object
    assert!(
        result.contains("pub struct ChatCompletionMessageToolCallChunkFunction"),
        "Should generate a struct for inline function object"
    );

    // Assert that the function field uses the generated type
    assert!(
        result.contains("pub function: Option<ChatCompletionMessageToolCallChunkFunction>"),
        "Function field should use the generated type"
    );

    // Assert that serde_json::Value is NOT used
    assert!(
        !result.contains("pub function: Option<serde_json::Value>"),
        "Function field should NOT fall back to serde_json::Value"
    );

    // Assert the generated struct has the expected fields
    assert!(
        result.contains("pub arguments: Option<String>")
            && result.contains("pub name: Option<String>"),
        "Generated function struct should have arguments and name fields"
    );
}

#[test]
fn test_inline_object_with_nullable_fields() {
    // Test case with nullable fields in inline object
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ResponseWithNullable": {
                    "type": "object",
                    "properties": {
                        "data": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string"},
                                    "optional_field": {
                                        "type": "string",
                                        "nullable": true
                                    },
                                    "required_field": {"type": "string"}
                                },
                                "required": ["id", "required_field"]
                            }
                        }
                    },
                    "required": ["data"]
                }
            }
        }
    });

    let result = test_generation("inline_object_nullable_test", spec).expect("Generation failed");

    // Assert that the data item type was generated
    assert!(
        result.contains("pub struct ResponseWithNullableDataItem")
            || result.contains("pub struct ResponseWithNullableItem"),
        "Should generate a struct for data array items"
    );

    // Check that nullable field is properly typed as Option
    assert!(
        result.contains("pub optional_field: Option<String>"),
        "Nullable field should be Option<String>"
    );

    // Check that required field is not Option
    assert!(
        result.contains("pub required_field: String"),
        "Required field should be String, not Option<String>"
    );
}
