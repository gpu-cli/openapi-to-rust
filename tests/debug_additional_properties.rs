#![cfg(feature = "test-helpers")]

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn debug_additional_properties_output() {
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

    let result = test_generation("debug_additional_properties", spec).expect("Generation failed");

    // Print the entire output to see what's being generated
    println!("=== GENERATED OUTPUT ===");
    println!("{result}");
    println!("=== END OUTPUT ===");

    // Check if it contains what we're looking for
    assert!(
        result.contains("BTreeMap<String, serde_json::Value>"),
        "FlexibleConfig should contain BTreeMap<String, serde_json::Value> for additionalProperties"
    );
    assert!(
        result.contains("#[serde(flatten)]"),
        "HashMap should be flattened with serde attribute"
    );
}
