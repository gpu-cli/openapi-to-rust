#![cfg(feature = "test-helpers")]

#[cfg(test)]
mod tests {
    use openapi_to_rust::test_helpers::*;
    use serde_json::json;

    #[test]
    fn test_create_response_input_field_properly_typed() {
        // Test that CreateResponse.input field generates a proper union type, not serde_json::Value
        let spec = json!({
            "openapi": "3.1.0",
            "info": {"title": "Test", "version": "1.0"},
            "components": {
                "schemas": {
                    "CreateResponse": {
                        "allOf": [
                            {
                                "$ref": "#/components/schemas/BaseProperties"
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "input": {
                                        "description": "Text or array inputs to the model",
                                        "oneOf": [
                                            {
                                                "type": "string",
                                                "description": "A text input",
                                                "title": "Text input"
                                            },
                                            {
                                                "type": "array",
                                                "description": "A list of input items",
                                                "title": "Input item list",
                                                "items": {
                                                    "$ref": "#/components/schemas/InputItem"
                                                }
                                            }
                                        ]
                                    }
                                },
                                "required": ["input"]
                            }
                        ]
                    },
                    "BaseProperties": {
                        "type": "object",
                        "properties": {
                            "model": {
                                "type": "string"
                            }
                        },
                        "required": ["model"]
                    },
                    "InputItem": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string",
                                "enum": ["text", "image"]
                            },
                            "content": {
                                "type": "string"
                            }
                        },
                        "required": ["type", "content"]
                    }
                }
            }
        });

        let result =
            test_generation("create_response_input_typing", spec).expect("Generation failed");

        // Check that CreateResponse was generated
        assert!(
            result.contains("pub struct CreateResponse"),
            "Should generate CreateResponse struct"
        );

        // Check that input field is NOT serde_json::Value
        assert!(
            !result.contains("pub input: serde_json::Value"),
            "Input field should NOT be serde_json::Value"
        );

        // Check that input field has a proper union type
        assert!(
            result.contains("pub input: CreateResponseInput"),
            "Input field should use a proper union type CreateResponseInput"
        );

        // Check that the union enum was generated
        assert!(
            result.contains("pub enum CreateResponseInput"),
            "Should generate CreateResponseInput enum"
        );

        // Check the union has the expected variants
        assert!(
            result.contains("#[serde(untagged)]"),
            "Union should be untagged"
        );
        assert!(
            result.contains("String(String)") || result.contains("TextInput(String)"),
            "Should have string variant"
        );
        assert!(
            result.contains("InputItemList(Vec<InputItem>)")
                || result.contains("Array(Vec<InputItem>)"),
            "Should have array variant"
        );
    }

    #[test]
    fn test_nested_allof_with_oneof_property() {
        // Test the exact pattern from OpenAI fixture
        let spec = json!({
            "openapi": "3.1.0",
            "info": {"title": "Test", "version": "1.0"},
            "components": {
                "schemas": {
                    "CreateResponse": {
                        "allOf": [
                            {
                                "$ref": "#/components/schemas/CreateModelResponseProperties"
                            },
                            {
                                "$ref": "#/components/schemas/ResponseProperties"
                            },
                            {
                                "properties": {
                                    "input": {
                                        "description": "Text, image, or file inputs to the model",
                                        "oneOf": [
                                            {
                                                "description": "A text input",
                                                "title": "Text input",
                                                "type": "string"
                                            },
                                            {
                                                "description": "A list of input items",
                                                "items": {
                                                    "$ref": "#/components/schemas/InputItem"
                                                },
                                                "title": "Input item list",
                                                "type": "array"
                                            }
                                        ]
                                    }
                                },
                                "required": ["model", "input"],
                                "type": "object"
                            }
                        ]
                    },
                    "CreateModelResponseProperties": {
                        "type": "object",
                        "properties": {
                            "model": {
                                "type": "string"
                            }
                        },
                        "required": ["model"]
                    },
                    "ResponseProperties": {
                        "type": "object",
                        "properties": {
                            "temperature": {
                                "type": "number",
                                "nullable": true
                            }
                        }
                    },
                    "InputItem": {
                        "type": "object",
                        "properties": {
                            "type": {
                                "type": "string"
                            },
                            "content": {
                                "type": "string"
                            }
                        },
                        "required": ["type", "content"]
                    }
                }
            }
        });

        let result = test_generation("nested_allof_oneof", spec).expect("Generation failed");

        // The same assertions as above
        assert!(result.contains("pub struct CreateResponse"));
        assert!(
            !result.contains("pub input: serde_json::Value"),
            "Input field should NOT be serde_json::Value - it should be properly typed"
        );
        assert!(
            result.contains("pub input: CreateResponseInput"),
            "Input field should use CreateResponseInput union type"
        );
        assert!(result.contains("pub enum CreateResponseInput"));
    }
}
