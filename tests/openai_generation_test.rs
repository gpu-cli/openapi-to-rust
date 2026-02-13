#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn test_openai_input_generates_ergonomic_types() {
        // Test the exact OpenAI API pattern
        let spec_json = json!({
            "openapi": "3.1.0",
            "info": {
                "title": "OpenAI API",
                "version": "2.3.0"
            },
            "components": {
                "schemas": {
                    "CreateChatCompletionRequest": {
                        "type": "object",
                        "properties": {
                            "messages": {
                                "type": "array",
                                "items": {
                                    "$ref": "#/components/schemas/ChatCompletionRequestMessage"
                                }
                            },
                            "model": {
                                "type": "string"
                            },
                            "store": {
                                "type": "boolean",
                                "nullable": true
                            }
                        },
                        "required": ["messages", "model"]
                    },
                    "ChatCompletionRequestMessage": {
                        "type": "object",
                        "properties": {
                            "role": {
                                "type": "string",
                                "enum": ["system", "user", "assistant"]
                            },
                            "content": {
                                "type": "string"
                            }
                        },
                        "required": ["role", "content"]
                    },
                    "CreateCompletionRequest": {
                        "type": "object",
                        "properties": {
                            "model": {
                                "type": "string"
                            },
                            "prompt": {
                                "description": "The prompt(s) to generate completions for",
                                "oneOf": [
                                    {
                                        "type": "string",
                                        "description": "A single prompt string"
                                    },
                                    {
                                        "type": "array",
                                        "description": "Multiple prompts",
                                        "items": {
                                            "type": "string"
                                        }
                                    }
                                ]
                            },
                            "max_tokens": {
                                "type": "integer",
                                "nullable": true
                            }
                        },
                        "required": ["model", "prompt"]
                    },
                    "CreateEmbeddingRequest": {
                        "type": "object",
                        "properties": {
                            "input": {
                                "description": "Input text to embed",
                                "oneOf": [
                                    {
                                        "type": "string",
                                        "description": "A single input string"
                                    },
                                    {
                                        "type": "array",
                                        "description": "Multiple input strings",
                                        "items": {
                                            "type": "string"
                                        }
                                    },
                                    {
                                        "type": "array",
                                        "description": "Token arrays",
                                        "items": {
                                            "type": "integer"
                                        }
                                    },
                                    {
                                        "type": "array",
                                        "description": "Arrays of token arrays",
                                        "items": {
                                            "type": "array",
                                            "items": {
                                                "type": "integer"
                                            }
                                        }
                                    }
                                ]
                            },
                            "model": {
                                "type": "string"
                            }
                        },
                        "required": ["input", "model"]
                    }
                }
            }
        });

        let mut analyzer = openapi_generator::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_generator::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        println!("Generated OpenAI types:\n{types_content}");

        // Check for ergonomic type names

        // 1. CreateCompletionRequest should have a nice Prompt union type
        assert!(
            types_content.contains("pub struct CreateCompletionRequest"),
            "Should generate CreateCompletionRequest struct"
        );
        assert!(
            types_content.contains("pub prompt: CreateCompletionRequestPrompt"),
            "Prompt field should use a proper union type, not serde_json::Value"
        );
        assert!(
            types_content.contains("pub enum CreateCompletionRequestPrompt"),
            "Should generate union enum for prompt"
        );
        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Prompt union should be untagged"
        );

        // 2. CreateEmbeddingRequest should have a nice Input union type
        assert!(
            types_content.contains("pub struct CreateEmbeddingRequest"),
            "Should generate CreateEmbeddingRequest struct"
        );
        assert!(
            types_content.contains("pub input: CreateEmbeddingRequestInput"),
            "Input field should use a proper union type"
        );
        assert!(
            types_content.contains("pub enum CreateEmbeddingRequestInput"),
            "Should generate union enum for input"
        );

        // Check the variants have good names
        assert!(
            types_content.contains("String(String)"),
            "Should have String variant"
        );
        assert!(
            types_content.contains("StringArray(Vec<String>)")
                || types_content.contains("Vec<String>"),
            "Should have string array variant"
        );
        assert!(
            types_content.contains("IntegerArray(Vec<i64>)") || types_content.contains("Vec<i64>"),
            "Should have integer array variant"
        );
        assert!(
            types_content.contains("IntegerArrayArray(Vec<Vec<i64>>)")
                || types_content.contains("Vec<Vec<i64>>"),
            "Should have array of integer arrays variant"
        );

        // 3. No serde_json::Value for oneOf fields
        assert!(
            !types_content.contains("pub prompt: serde_json::Value"),
            "Prompt should NOT be serde_json::Value"
        );
        assert!(
            !types_content.contains("pub input: serde_json::Value"),
            "Input should NOT be serde_json::Value"
        );

        // 4. Nullable fields should be Option<T>
        assert!(
            types_content.contains("pub store: Option<bool>"),
            "Nullable boolean should be Option<bool>"
        );
        assert!(
            types_content.contains("pub max_tokens: Option<i32>")
                || types_content.contains("pub max_tokens: Option<i64>"),
            "Nullable integer should be Option<i32/i64>"
        );
    }

    #[test]
    fn test_openai_streaming_response_types() {
        // Test streaming response types specifically
        let spec_json = json!({
            "openapi": "3.1.0",
            "info": {
                "title": "OpenAI Streaming API",
                "version": "1.0.0"
            },
            "components": {
                "schemas": {
                    "CreateChatCompletionStreamResponse": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string"
                            },
                            "object": {
                                "type": "string",
                                "enum": ["chat.completion.chunk"]
                            },
                            "created": {
                                "type": "integer"
                            },
                            "model": {
                                "type": "string"
                            },
                            "choices": {
                                "type": "array",
                                "items": {
                                    "$ref": "#/components/schemas/ChatCompletionStreamResponseChoice"
                                }
                            }
                        },
                        "required": ["id", "object", "created", "model", "choices"]
                    },
                    "ChatCompletionStreamResponseChoice": {
                        "type": "object",
                        "properties": {
                            "index": {
                                "type": "integer"
                            },
                            "delta": {
                                "$ref": "#/components/schemas/ChatCompletionStreamResponseDelta"
                            },
                            "finish_reason": {
                                "type": "string",
                                "nullable": true,
                                "enum": ["stop", "length", "tool_calls", "content_filter", "function_call"]
                            }
                        },
                        "required": ["index", "delta"]
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
                                "enum": ["system", "user", "assistant", "tool"]
                            }
                        }
                    }
                }
            }
        });

        let mut analyzer = openapi_generator::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_generator::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        // Check streaming types have nice names
        assert!(
            types_content.contains("pub struct CreateChatCompletionStreamResponse"),
            "Should generate streaming response struct"
        );
        assert!(
            types_content.contains("pub struct ChatCompletionStreamResponseChoice"),
            "Should generate choice struct"
        );
        assert!(
            types_content.contains("pub struct ChatCompletionStreamResponseDelta"),
            "Should generate delta struct"
        );

        // Enums should be properly typed
        assert!(
            types_content.contains("pub object: CreateChatCompletionStreamResponseObject")
                || types_content.contains("pub object: String"),
            "Object field should be typed"
        );

        // Nullable fields
        assert!(
            types_content.contains("pub content: Option<String>"),
            "Nullable content should be Option<String>"
        );
        assert!(
            types_content.contains("pub finish_reason: Option<"),
            "Nullable finish_reason should be Option<T>"
        );
    }
}
