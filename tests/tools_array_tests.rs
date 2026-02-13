//! Tests for tools array generation
//!
//! These tests ensure that tools arrays are properly typed as union types
//! instead of falling back to serde_json::Value

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn test_tools_array_uses_proper_union_types() {
    // Test that tools arrays in CreateMessageParams use proper union types for tools
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "CreateMessageParams": {
                    "type": "object",
                    "properties": {
                        "model": {"type": "string"},
                        "messages": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/Message"}
                        },
                        "tools": {
                            "type": "array",
                            "items": {
                                "oneOf": [
                                    {"$ref": "#/components/schemas/Tool"},
                                    {"$ref": "#/components/schemas/BashTool"}
                                ]
                            },
                            "description": "Tools available to the model"
                        }
                    },
                    "required": ["model", "messages"]
                },
                "Message": {
                    "type": "object",
                    "properties": {
                        "role": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "required": ["role", "content"]
                },
                "Tool": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "custom"},
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "input_schema": {"$ref": "#/components/schemas/InputSchema"}
                    },
                    "required": ["name", "input_schema"]
                },
                "BashTool": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "bash"},
                        "name": {"type": "string"}
                    },
                    "required": ["name"]
                },
                "InputSchema": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string"},
                        "properties": {"type": "object"}
                    }
                }
            }
        }
    });

    let result = test_generation("tools_array_union_test", spec).expect("Generation failed");

    // Assert that a union type was generated for the tools array
    assert!(
        result.contains("pub enum CreateMessageParamsToolsItem") || result.contains("ToolsItem"),
        "Should generate a union enum for tools array items"
    );

    // Assert that Tool and BashTool types were generated
    assert!(
        result.contains("pub struct Tool"),
        "Tool struct should be generated"
    );
    assert!(
        result.contains("pub struct BashTool"),
        "BashTool struct should be generated"
    );

    // Check that the tools field uses Vec of the union type, not serde_json::Value
    assert!(
        !result.contains("Vec<serde_json::Value>"),
        "Tools field should NOT use serde_json::Value"
    );

    // For discriminated unions, we should see serde tag attributes or proper enum variants
    assert!(
        result.contains("#[serde(tag = \"type\")]")
            || result.contains("Custom")
            || result.contains("Bash")
            || result.contains("Tool(Tool)")
            || result.contains("BashTool(BashTool)"),
        "Union should have proper enum variants or discriminator attributes"
    );
}

#[test]
fn test_beta_tools_array_uses_proper_union_types() {
    // Test that Beta tools arrays use proper union types
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "BetaCreateMessageParams": {
                    "type": "object",
                    "properties": {
                        "model": {"type": "string"},
                        "messages": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/Message"}
                        },
                        "tools": {
                            "type": "array",
                            "items": {
                                "oneOf": [
                                    {"$ref": "#/components/schemas/BetaTool"},
                                    {"$ref": "#/components/schemas/BetaComputerUseTool"}
                                ]
                            },
                            "description": "Beta tools available to the model"
                        }
                    },
                    "required": ["model", "messages"]
                },
                "Message": {
                    "type": "object",
                    "properties": {
                        "role": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "required": ["role", "content"]
                },
                "BetaTool": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "custom"},
                        "name": {"type": "string"},
                        "description": {"type": "string"},
                        "input_schema": {"$ref": "#/components/schemas/BetaInputSchema"}
                    },
                    "required": ["name", "input_schema"]
                },
                "BetaComputerUseTool": {
                    "type": "object",
                    "properties": {
                        "type": {"const": "computer_20241022"},
                        "name": {"type": "string"},
                        "display_width_px": {"type": "integer"},
                        "display_height_px": {"type": "integer"}
                    },
                    "required": ["name"]
                },
                "BetaInputSchema": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string"},
                        "properties": {"type": "object"}
                    }
                }
            }
        }
    });

    let result = test_generation("beta_tools_array_union_test", spec).expect("Generation failed");

    // The tools field should NOT use serde_json::Value
    assert!(
        !result.contains("Vec<serde_json::Value>"),
        "Beta tools field should not use serde_json::Value"
    );

    // Should have a union type for tools
    assert!(
        result.contains("BetaCreateMessageParamsToolsItem")
            || result.contains("enum") && result.contains("BetaTool"),
        "Should have union type for beta tools"
    );

    // Verify BetaTool and BetaComputerUseTool structs are generated
    assert!(
        result.contains("pub struct BetaTool"),
        "BetaTool type should be generated"
    );
    assert!(
        result.contains("pub struct BetaComputerUseTool"),
        "BetaComputerUseTool type should be generated"
    );
}

#[test]
fn test_simple_array_with_single_tool_type() {
    // Test a simpler case where tools array has only one type
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "SimpleRequest": {
                    "type": "object",
                    "properties": {
                        "tools": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/SimpleTool"}
                        }
                    }
                },
                "SimpleTool": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "description": {"type": "string"}
                    },
                    "required": ["name"]
                }
            }
        }
    });

    let result = test_generation("simple_tools_array_test", spec).expect("Generation failed");

    // The tools field should use Vec<SimpleTool>, not serde_json::Value
    assert!(
        !result.contains("serde_json::Value"),
        "Simple tools field should not use serde_json::Value"
    );

    assert!(
        result.contains("Vec<SimpleTool>"),
        "Simple tools field should use Vec<SimpleTool>"
    );

    // Verify SimpleTool struct is generated
    assert!(
        result.contains("pub struct SimpleTool"),
        "SimpleTool type should be generated"
    );
}
