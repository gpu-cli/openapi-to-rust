#[cfg(test)]
mod tests {
    use openapi_to_rust::test_helpers::*;
    use serde_json::json;

    #[test]
    fn test_oneof_with_inline_string_and_array() {
        // Create minimal OpenAPI spec with oneOf containing inline types
        let spec_json = minimal_spec(json!({
            "CreateRequest": {
                "type": "object",
                "properties": {
                    "input": {
                        "description": "Input that can be either a string or an array",
                        "oneOf": [
                            {
                                "type": "string",
                                "description": "Simple text input"
                            },
                            {
                                "type": "array",
                                "description": "Array of input items",
                                "items": {
                                    "$ref": "#/components/schemas/InputItem"
                                }
                            }
                        ]
                    }
                },
                "required": ["input"]
            },
            "InputItem": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string"
                    }
                },
                "required": ["text"]
            }
        }));

        // Generate code from the spec
        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();

        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        println!("Generated types.rs:\n{types_content}");

        // Check that CreateRequest was generated
        assert!(types_content.contains("pub struct CreateRequest"));

        // The key assertion: input should NOT be serde_json::Value
        assert!(
            !types_content.contains("pub input: serde_json::Value"),
            "Input field should not be serde_json::Value"
        );

        // Should generate a union type for the input
        assert!(
            types_content.contains("pub enum CreateRequestInput")
                || types_content.contains("pub input: CreateRequestInput"),
            "Should generate a union type for the input field"
        );

        // The union should have String and Array variants
        if types_content.contains("pub enum CreateRequestInput") {
            assert!(
                types_content.contains("String(String)"),
                "Union should have String variant"
            );
            assert!(
                types_content.contains("Array(Vec<InputItem>)")
                    || types_content.contains("InputItemArray(Vec<InputItem>)"),
                "Union should have Array variant"
            );
        }
    }
}
