#[cfg(test)]
mod tests {
    use openapi_to_rust::test_helpers::*;
    use serde_json::json;

    #[test]
    fn test_enum_generation_for_properties() {
        let spec = minimal_spec(json!({
            "RoleEnum": {
                "type": "string",
                "enum": ["assistant"],
                "const": "assistant"
            },
            "SimpleMessage": {
                "type": "object",
                "properties": {
                    "role": {
                        "type": "string",
                        "enum": ["assistant"],
                        "const": "assistant",
                        "default": "assistant"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["active", "inactive", "pending"]
                    }
                },
                "required": ["role", "status"]
            }
        }));

        let result = test_generation("enum_properties", spec).expect("Generation failed");

        // Assert that RoleEnum was generated as a single-variant enum
        assert!(
            result.contains("pub enum RoleEnum"),
            "RoleEnum should be generated as an enum"
        );
        assert!(
            result.contains("pub enum SimpleMessageRole"),
            "SimpleMessageRole enum should be generated for role property"
        );
        assert!(
            result.contains("pub enum SimpleMessageStatus"),
            "SimpleMessageStatus enum should be generated for status property"
        );

        // Assert that the role property uses the enum type, not String
        assert!(
            !result.contains("pub role: String"),
            "role property should NOT be String"
        );
        assert!(
            result.contains("pub role: SimpleMessageRole"),
            "role property should use SimpleMessageRole enum"
        );

        // Assert that status property uses the enum type, not String
        assert!(
            !result.contains("pub status: String"),
            "status property should NOT be String"
        );
        assert!(
            result.contains("pub status: SimpleMessageStatus"),
            "status property should use SimpleMessageStatus enum"
        );

        // Verify the enum variants are correct
        assert!(
            result.contains("#[serde(rename = \"assistant\")]"),
            "RoleEnum should have assistant variant"
        );
        assert!(
            result.contains("#[serde(rename = \"active\")]"),
            "StatusEnum should have active variant"
        );
        assert!(
            result.contains("#[serde(rename = \"inactive\")]"),
            "StatusEnum should have inactive variant"
        );
        assert!(
            result.contains("#[serde(rename = \"pending\")]"),
            "StatusEnum should have pending variant"
        );

        // Verify default trait implementation
        assert!(result.contains("Default"), "Enums should derive Default");
        assert!(
            result.contains("#[default]"),
            "Enums should have a default variant marked"
        );
    }
}
