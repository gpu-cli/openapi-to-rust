#![cfg(feature = "test-helpers")]

#[cfg(test)]
mod tests {
    use openapi_to_rust::test_helpers::*;
    use serde_json::json;

    #[test]
    fn test_oneof_string_or_array_inline() {
        // Test oneOf with inline string and array types
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

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        // Should generate untagged union
        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Should generate untagged union"
        );
        assert!(
            types_content.contains("pub enum CreateRequestInput"),
            "Should generate union enum for input"
        );
        assert!(
            types_content.contains("String(String)"),
            "Should have String variant"
        );
        assert!(
            types_content.contains("Vec<InputItem>"),
            "Should have array variant with proper type"
        );

        // Should NOT fall back to serde_json::Value
        assert!(
            !types_content.contains("pub input: serde_json::Value"),
            "Should not use serde_json::Value for input field"
        );
    }

    #[test]
    fn test_oneof_multiple_primitives() {
        // Test oneOf with multiple primitive types
        let spec_json = minimal_spec(json!({
            "FlexibleValue": {
                "type": "object",
                "properties": {
                    "value": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "number"},
                            {"type": "boolean"},
                            {"type": "null"}
                        ]
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Should generate untagged union"
        );
        assert!(
            types_content.contains("String(String)"),
            "Should have String variant"
        );
        assert!(
            types_content.contains("Number(f64)"),
            "Should have Number variant"
        );
        assert!(
            types_content.contains("Boolean(bool)"),
            "Should have Boolean variant"
        );
    }

    #[test]
    fn test_oneof_object_variants() {
        // Test oneOf with inline object schemas
        let spec_json = minimal_spec(json!({
            "Message": {
                "type": "object",
                "properties": {
                    "content": {
                        "oneOf": [
                            {
                                "type": "object",
                                "properties": {
                                    "text": {"type": "string"}
                                },
                                "required": ["text"]
                            },
                            {
                                "type": "object",
                                "properties": {
                                    "image_url": {"type": "string"},
                                    "detail": {"type": "string"}
                                },
                                "required": ["image_url"]
                            }
                        ]
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        println!("Object variants test output:\n{types_content}");

        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Should generate untagged union"
        );
        assert!(
            types_content.contains("pub enum MessageContent"),
            "Should generate union enum"
        );
        // Should create inline types for the objects
        assert!(
            types_content.contains("InlineVariant") || types_content.contains("MessageContent"),
            "Should create inline variant types or embed them in the union"
        );
    }

    #[test]
    fn test_oneof_mixed_refs_and_inline() {
        // Test oneOf with mix of references and inline schemas
        let spec_json = minimal_spec(json!({
            "Request": {
                "type": "object",
                "properties": {
                    "data": {
                        "oneOf": [
                            {"$ref": "#/components/schemas/User"},
                            {"$ref": "#/components/schemas/Team"},
                            {"type": "string"},
                            {
                                "type": "array",
                                "items": {"type": "string"}
                            }
                        ]
                    }
                }
            },
            "User": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "email": {"type": "string"}
                }
            },
            "Team": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "members": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Should generate untagged union"
        );
        assert!(
            types_content.contains("User(User)"),
            "Should have User variant"
        );
        assert!(
            types_content.contains("Team(Team)"),
            "Should have Team variant"
        );
        assert!(
            types_content.contains("String(String)"),
            "Should have String variant"
        );
        assert!(
            types_content.contains("Vec<String>"),
            "Should have string array variant"
        );
    }

    #[test]
    fn test_oneof_with_discriminator_still_works() {
        // Ensure discriminated unions still work correctly
        let spec_json = minimal_spec(json!({
            "Pet": {
                "oneOf": [
                    {"$ref": "#/components/schemas/Cat"},
                    {"$ref": "#/components/schemas/Dog"}
                ],
                "discriminator": {
                    "propertyName": "type"
                }
            },
            "Cat": {
                "type": "object",
                "properties": {
                    "type": {"type": "string", "enum": ["cat"]},
                    "meow": {"type": "boolean"}
                },
                "required": ["type"]
            },
            "Dog": {
                "type": "object",
                "properties": {
                    "type": {"type": "string", "enum": ["dog"]},
                    "bark": {"type": "boolean"}
                },
                "required": ["type"]
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        // Should generate tagged union for discriminated oneOf
        assert!(
            types_content.contains("#[serde(tag = \"type\")]"),
            "Should generate tagged union for discriminated oneOf"
        );
        assert!(
            !types_content.contains("#[serde(untagged)]"),
            "Should NOT generate untagged union when discriminator present"
        );
    }

    #[test]
    fn test_oneof_deeply_nested() {
        // Test oneOf in deeply nested properties
        let spec_json = minimal_spec(json!({
            "Root": {
                "type": "object",
                "properties": {
                    "level1": {
                        "type": "object",
                        "properties": {
                            "level2": {
                                "type": "object",
                                "properties": {
                                    "value": {
                                        "oneOf": [
                                            {"type": "string"},
                                            {"type": "number"}
                                        ]
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        println!("Deeply nested test output:\n{types_content}");

        // Should generate union for deeply nested oneOf
        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Should generate untagged union for nested oneOf"
        );
        // The union name should reflect the nesting context - could be various naming patterns
        assert!(
            types_content.contains("Value") && types_content.contains("pub enum"),
            "Should generate a union enum with contextual naming"
        );
    }

    #[test]
    fn test_oneof_nullable_handling() {
        // Test oneOf with nullable option
        let spec_json = minimal_spec(json!({
            "Response": {
                "type": "object",
                "properties": {
                    "result": {
                        "oneOf": [
                            {"type": "string"},
                            {"type": "null"}
                        ]
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        // The non-null variant should reduce to String — directly as Option<String>,
        // through a type alias (`pub type X = String;`), or as an untagged enum that
        // contains a String variant. The {"type": "null"} branch must not produce a
        // `serde_json::Value` / `SerdeJsonValue` variant.
        assert!(
            types_content.contains("Option<String>")
                || types_content.contains("= String;")
                || types_content.contains("#[serde(untagged)]"),
            "Should handle nullable oneOf appropriately, got:\n{types_content}"
        );
        assert!(
            !types_content.contains("SerdeJsonValue"),
            "null variant must not become a SerdeJsonValue variant, got:\n{types_content}"
        );
    }

    // Regression for https://github.com/gpu-cli/openapi-to-rust/issues/7:
    // anyOf with {"type": "null"} among 3+ variants used to panic with
    // `"()" is not a valid Ident` because the null branch produced a
    // `Primitive { rust_type: "()" }` type alias.
    #[test]
    fn test_anyof_null_among_multiple_variants_does_not_panic() {
        let spec_json = minimal_spec(json!({
            "AnyOfNullMixed": {
                "type": "object",
                "properties": {
                    "value": {
                        "anyOf": [
                            {"type": "string"},
                            {"type": "number"},
                            {"type": "null"}
                        ]
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        assert!(
            !types_content.contains("= ()"),
            "null variant must not produce a `()` type alias, got:\n{types_content}"
        );
        assert!(
            !types_content.contains("SerdeJsonValue"),
            "null variant must not become SerdeJsonValue, got:\n{types_content}"
        );
    }

    // Regression for https://github.com/gpu-cli/openapi-to-rust/issues/7:
    // oneOf with an array variant used to emit `Vec<SerdeJsonValue>` because
    // the generator mangled the inner `serde_json::Value` path into an ident.
    #[test]
    fn test_oneof_array_with_empty_items_emits_real_type() {
        let spec_json = minimal_spec(json!({
            "OneOfArrayOption": {
                "type": "object",
                "properties": {
                    "value": {
                        "oneOf": [
                            {"type": "array", "items": {}},
                            {"type": "string"},
                            {"type": "number"}
                        ]
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        assert!(
            !types_content.contains("SerdeJsonValue"),
            "Vec inner type leaked as SerdeJsonValue, got:\n{types_content}"
        );
        assert!(
            types_content.contains("Vec<serde_json::Value>"),
            "expected Vec<serde_json::Value> for array with `items: {{}}`, got:\n{types_content}"
        );
    }

    // Regression for https://github.com/gpu-cli/openapi-to-rust/issues/7:
    // {"type": "null"} inside oneOf with 3+ variants must be filtered out, not
    // emitted as a phantom `SerdeJsonValue(SerdeJsonValue)` variant.
    #[test]
    fn test_oneof_null_among_multiple_variants_is_filtered() {
        let spec_json = minimal_spec(json!({
            "NullOneOfMixed": {
                "type": "object",
                "properties": {
                    "value": {
                        "oneOf": [
                            {"type": "null"},
                            {"type": "string"},
                            {"type": "integer"}
                        ]
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        assert!(
            !types_content.contains("SerdeJsonValue"),
            "null variant leaked through as SerdeJsonValue, got:\n{types_content}"
        );
        assert!(
            types_content.contains("String(String)"),
            "expected String variant in untagged enum, got:\n{types_content}"
        );
        assert!(
            types_content.contains("Integer(i64)"),
            "expected Integer variant in untagged enum, got:\n{types_content}"
        );
    }

    #[test]
    fn test_openai_style_input_field() {
        // Test the exact pattern used by OpenAI's CreateResponse input field
        let spec_json = minimal_spec(json!({
            "CreateResponse": {
                "type": "object",
                "properties": {
                    "input": {
                        "description": "Text, image, or file inputs to the model",
                        "oneOf": [
                            {
                                "type": "string",
                                "description": "A text input to the model"
                            },
                            {
                                "type": "array",
                                "description": "A list of input items",
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
                    "type": {
                        "type": "string",
                        "enum": ["text", "image", "file"]
                    },
                    "text": {
                        "type": "string",
                        "description": "Text content"
                    },
                    "image_url": {
                        "type": "string",
                        "description": "Image URL"
                    }
                },
                "required": ["type"]
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        // Should generate proper typed union, not serde_json::Value
        assert!(
            !types_content.contains("pub input: serde_json::Value"),
            "Input field should NOT be serde_json::Value"
        );
        assert!(
            types_content.contains("pub input: CreateResponseInput"),
            "Input field should use generated union type"
        );
        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Should generate untagged union"
        );
        assert!(
            types_content.contains("String(String)"),
            "Should have String variant"
        );
        assert!(
            types_content.contains("Vec<InputItem>"),
            "Should have properly typed array variant"
        );
    }

    #[test]
    fn test_oneof_array_of_unions() {
        // Test array containing oneOf items
        let spec_json = minimal_spec(json!({
            "Container": {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {
                            "oneOf": [
                                {"type": "string"},
                                {"type": "number"},
                                {
                                    "type": "object",
                                    "properties": {
                                        "id": {"type": "string"}
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }));

        let mut analyzer = openapi_to_rust::SchemaAnalyzer::new(spec_json.clone()).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = openapi_to_rust::CodeGenerator::new(Default::default());
        let types_content = generator.generate(&mut analysis).unwrap();

        // Should generate array of union type
        assert!(types_content.contains("Vec<"), "Should generate Vec type");
        assert!(
            types_content.contains("#[serde(untagged)]"),
            "Should generate untagged union for array items"
        );
    }
}
