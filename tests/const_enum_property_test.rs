#[cfg(test)]
mod tests {
    use openapi_generator::test_helpers::*;
    use serde_json::json;

    #[test]
    fn test_const_enum_in_object_property() {
        // This test reproduces the exact issue with the role field in Message
        let spec = minimal_spec(json!({
            "Message": {
                "type": "object",
                "title": "Message",
                "properties": {
                    "content": {
                        "description": "The content of the message.",
                        "title": "Content",
                        "oneOf": [
                            {
                                "type": "string",
                                "title": "Text content",
                                "description": "Text content of the message."
                            },
                            {
                                "type": "array",
                                "title": "Array of content blocks",
                                "items": {
                                    "type": "object"
                                }
                            }
                        ]
                    },
                    "id": {
                        "type": "string",
                        "description": "Unique object identifier."
                    },
                    "role": {
                        "const": "assistant",
                        "default": "assistant",
                        "description": "Conversational role of the generated message.\n\nThis will always be `\"assistant\"`.",
                        "enum": ["assistant"],
                        "title": "Role",
                        "type": "string"
                    },
                    "model": {
                        "type": "string",
                        "description": "The model that was used to generate the message."
                    }
                },
                "required": ["content", "id", "role", "model"]
            }
        }));

        let result = test_generation("const_enum_property", spec).expect("Generation failed");

        // Assert that MessageRole enum was generated for the role property
        assert!(
            result.contains("pub enum MessageRole"),
            "MessageRole enum should be generated for role property with const+enum"
        );

        // Assert that the role field uses the enum type, not String
        assert!(
            !result.contains("pub role: String"),
            "role property should NOT be String when it has const+enum values"
        );
        assert!(
            result.contains("pub role: MessageRole"),
            "role property should use MessageRole enum type"
        );

        // Verify the enum has the correct variant
        assert!(
            result.contains("#[serde(rename = \"assistant\")]"),
            "MessageRole enum should have assistant variant"
        );
        assert!(
            result.contains("Assistant"),
            "MessageRole enum should have Assistant variant"
        );

        // Verify that it's marked as the default variant since it has a default value
        assert!(
            result.contains("#[default]"),
            "MessageRole enum should have a default variant since the property has default: 'assistant'"
        );

        // Verify the struct has the proper serde attribute for the default
        assert!(
            result.contains("#[serde(default)]"),
            "role field should have serde(default) attribute"
        );

        // Verify other fields are generated correctly
        assert!(
            result.contains("pub id: String"),
            "id field should be String"
        );
        assert!(
            result.contains("pub model: String"),
            "model field should be String"
        );
        assert!(
            result.contains("pub content: "),
            "content field should exist"
        );
    }
}
