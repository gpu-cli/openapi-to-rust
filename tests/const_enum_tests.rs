#![cfg(feature = "test-helpers")]

#[cfg(test)]
mod tests {
    use openapi_to_rust::test_helpers::*;
    use serde_json::json;

    #[test]
    fn test_const_with_enum_single_value() {
        let spec = minimal_spec(json!({
            "Message": {
                "type": "object",
                "properties": {
                    "role": {
                        "const": "assistant",
                        "default": "assistant",
                        "description": "Conversational role of the generated message.\n\nThis will always be `\"assistant\"`.",
                        "enum": ["assistant"],
                        "title": "Role",
                        "type": "string"
                    },
                    "content": {
                        "type": "string"
                    }
                },
                "required": ["role", "content"]
            }
        }));

        let result = test_generation("const_enum_test", spec).expect("Generation failed");

        // Assert that MessageRole enum was generated for the role property
        assert!(
            result.contains("pub enum MessageRole"),
            "MessageRole enum should be generated for role property with const+enum"
        );

        // Assert that the Message struct uses the enum type
        assert!(
            !result.contains("pub role: String"),
            "role field should NOT be String"
        );
        assert!(
            result.contains("pub role: MessageRole"),
            "role field should use MessageRole enum"
        );

        // Assert the enum has correct structure
        assert!(
            result.contains("#[serde(rename = \"assistant\")]"),
            "MessageRole should have serde rename for assistant"
        );
        assert!(
            result.contains("#[default]"),
            "MessageRole should have default variant"
        );
    }

    #[test]
    fn test_anyof_with_const_values() {
        let spec = minimal_spec(json!({
            "Model": {
                "type": null,
                "title": "Model",
                "description": "The model that will complete your prompt.",
                "anyOf": [
                    { "type": "string" },
                    {
                        "const": "claude-3-5-sonnet-latest",
                        "description": "Most capable model"
                    },
                    {
                        "const": "claude-3-5-haiku-latest",
                        "description": "Fastest model"
                    }
                ]
            },
            "Request": {
                "type": "object",
                "properties": {
                    "model": {
                        "$ref": "#/components/schemas/Model"
                    }
                }
            }
        }));

        let result = test_generation("model_anyof_test", spec).expect("Generation failed");

        // Assert that Model is generated as an enum (either union or string)
        assert!(
            result.contains("pub enum Model"),
            "Model should be generated as an enum"
        );

        // The Model should handle both const values and arbitrary strings
        // It could be either:
        // 1. An untagged union enum with String variant
        // 2. A string enum with known values (if only consts were provided)
        let has_string_variant = result.contains("String(String)");
        let has_string_enum_values =
            result.contains("Claude35SonnetLatest") || result.contains("Claude35HaikuLatest");

        assert!(
            has_string_variant || has_string_enum_values,
            "Model enum should either have String variant or specific value variants"
        );
    }

    #[test]
    fn test_better_const_enum_generation() {
        // This test shows what we'd ideally like to generate
        let spec = minimal_spec(json!({
            // For a field with const + enum with single value
            "SingleValueEnum": {
                "type": "string",
                "const": "fixed_value",
                "enum": ["fixed_value"]
            },
            // For anyOf with const values + open string
            "FlexibleEnum": {
                "anyOf": [
                    { "type": "string" },
                    { "const": "known_value_1" },
                    { "const": "known_value_2" }
                ]
            }
        }));

        let result = test_generation("ideal_const_enum", spec).expect("Generation failed");

        // Assert SingleValueEnum is generated as a proper enum
        assert!(
            result.contains("pub enum SingleValueEnum"),
            "SingleValueEnum should be generated as an enum"
        );
        assert!(
            result.contains("#[serde(rename = \"fixed_value\")]"),
            "SingleValueEnum should have serde rename"
        );
        assert!(
            result.contains("FixedValue"),
            "SingleValueEnum should have FixedValue variant"
        );

        // Assert FlexibleEnum handles both const values and open strings
        assert!(
            result.contains("pub enum FlexibleEnum"),
            "FlexibleEnum should be generated as an enum"
        );

        // FlexibleEnum should have a Custom variant for unknown values
        assert!(
            result.contains("Custom(String)") || result.contains("Other(String)"),
            "FlexibleEnum should have a Custom/Other(String) variant"
        );

        // FlexibleEnum should have custom Serialize/Deserialize implementations
        assert!(
            result.contains("impl<'de> serde::Deserialize<'de> for FlexibleEnum")
                || result.contains("impl serde::Serialize for FlexibleEnum"),
            "FlexibleEnum should have custom Serialize/Deserialize implementation"
        );
    }
}
