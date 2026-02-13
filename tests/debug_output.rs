use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn debug_empty_object_output() {
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

    let result = test_generation("debug_empty_object", spec).expect("Generation failed");

    // Print the entire output to see what's being generated
    println!("=== GENERATED OUTPUT ===");
    println!("{result}");
    println!("=== END OUTPUT ===");

    // Check if it contains what we're looking for
    if !result.contains("pub properties: Option<serde_json::Value>") {
        println!("MISSING: pub properties: Option<serde_json::Value>");
    }
    if result.contains("pub struct InputSchemaProperties") {
        println!("FOUND UNWANTED: pub struct InputSchemaProperties");
    }
}
