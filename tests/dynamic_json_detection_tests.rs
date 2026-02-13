use openapi_generator::test_helpers::*;
use serde_json::json;

#[test]
fn test_empty_object_becomes_json_value() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "InputSchema": {
                    "type": "object",
                    "properties": {
                        "properties": {
                            "anyOf": [
                                { "type": "object" },
                                { "type": "null" }
                            ],
                            "title": "Properties"
                        }
                    },
                    "required": ["properties"]
                }
            }
        }
    });

    let result = test_generation("empty_object_json_value_test", spec).expect("Generation failed");

    // The properties field should be serde_json::Value, not an empty struct
    assert!(result.contains("pub properties: Option<serde_json::Value>"));
    assert!(!result.contains("pub struct InputSchemaProperties"));
}

#[test]
fn test_tool_input_empty_object() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ToolUseBlock": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "input": {
                            "title": "Input",
                            "type": "object"
                        },
                        "name": { "type": "string" }
                    },
                    "required": ["id", "input", "name"]
                }
            }
        }
    });

    let result = test_generation("tool_input_empty_object_test", spec).expect("Generation failed");

    // The input field should be serde_json::Value
    assert!(result.contains("pub input: serde_json::Value"));
    assert!(!result.contains("pub struct ToolUseBlockInput"));
}

#[test]
fn test_object_with_additional_properties_only() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "FlexibleConfig": {
                    "type": "object",
                    "additionalProperties": true
                }
            }
        }
    });

    let result =
        test_generation("additional_properties_only_test", spec).expect("Generation failed");

    // Should generate a HashMap for additional properties
    assert!(result.contains("BTreeMap<String, serde_json::Value>"));
}

#[test]
fn test_structured_object_remains_typed() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "name": { "type": "string" },
                        "email": { "type": "string" }
                    },
                    "required": ["id", "name"]
                }
            }
        }
    });

    let result = test_generation("structured_object_test", spec).expect("Generation failed");

    // Should generate a proper struct with fields
    assert!(result.contains("pub struct User"));
    assert!(result.contains("pub id: String"));
    assert!(result.contains("pub name: String"));
    assert!(result.contains("pub email: Option<String>"));
    // Ensure no serde_json::Value for this well-defined struct
    let user_section = result.split("pub struct User").nth(1).unwrap_or("");
    let next_struct = user_section
        .find("pub struct")
        .unwrap_or(user_section.len());
    let user_code = &user_section[..next_struct];
    assert!(!user_code.contains("serde_json::Value"));
}

#[test]
fn test_object_with_constraints_remains_typed() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ConstrainedObject": {
                    "type": "object",
                    "minProperties": 1,
                    "maxProperties": 5
                }
            }
        }
    });

    let result = test_generation("constrained_object_test", spec).expect("Generation failed");

    // Should generate a struct even though no properties are defined
    // because it has constraints
    assert!(result.contains("pub struct ConstrainedObject"));
}

#[test]
fn test_nested_empty_objects() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Container": {
                    "type": "object",
                    "properties": {
                        "data": {
                            "anyOf": [
                                { "type": "object" },
                                { "type": "null" }
                            ]
                        },
                        "metadata": {
                            "type": "object"
                        }
                    }
                }
            }
        }
    });

    let result = test_generation("nested_empty_objects_test", spec).expect("Generation failed");

    // Both data and metadata should be serde_json::Value
    assert!(result.contains("pub data: Option<serde_json::Value>"));
    assert!(result.contains("pub metadata: Option<serde_json::Value>"));
}

#[test]
fn test_object_with_only_type_property() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "TypedEvent": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "const": "event.created"
                        }
                    },
                    "required": ["type"]
                }
            }
        }
    });

    let result = test_generation("type_property_only_test", spec).expect("Generation failed");

    // Should still generate a struct because the type property is structural
    assert!(result.contains("pub struct TypedEvent"));
    // But it might be used in a discriminated union context
}
