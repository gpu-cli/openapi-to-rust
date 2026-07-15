#![cfg(feature = "test-helpers")]

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn debug_type_property_output() {
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

    let result = test_generation("debug_test", spec).expect("Generation failed");

    // Print the entire output to see what's being generated
    println!("=== GENERATED OUTPUT ===");
    println!("{result}");
    println!("=== END OUTPUT ===");

    // Basic assertion to ensure the type was generated
    assert!(result.contains("TypedEvent"));
    assert!(result.contains("type") || result.contains("type_"));
}
