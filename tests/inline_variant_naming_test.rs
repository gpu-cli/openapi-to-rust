//! Test for improved inline variant naming in untagged unions

use openapi_generator::test_helpers::*;
use serde_json::json;

#[test]
fn test_inline_variant_naming() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "MessageContent": {
                    "title": "MessageContent",
                    "anyOf": [
                        {
                            "type": "string",
                            "description": "Plain text content"
                        },
                        {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/ContentBlock"
                            }
                        }
                    ]
                },
                "ContentBlock": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string" },
                        "text": { "type": "string" }
                    },
                    "required": ["type", "text"]
                },
                "ConfigValue": {
                    "title": "ConfigValue",
                    "anyOf": [
                        { "type": "string" },
                        { "type": "integer" },
                        { "type": "boolean" },
                        { "type": "number" }
                    ]
                }
            }
        }
    });

    let result = test_generation("inline_variant_naming", spec).expect("Generation failed");

    // Check that inline variants have descriptive names
    assert!(
        result.contains("MessageContentString") || result.contains("String(String)"),
        "First string variant should be named MessageContentString or be a String variant"
    );
    assert!(
        !result.contains("InlineVariant0"),
        "Should not contain generic InlineVariant0 names"
    );

    // Check ConfigValue variants - the generator may use different naming strategies
    assert!(
        result.contains("ConfigValue"),
        "Should have ConfigValue enum"
    );
    assert!(
        result.contains("String") || result.contains("ConfigValueString"),
        "String variant should be present"
    );
    assert!(
        result.contains("Integer")
            || result.contains("ConfigValueInteger")
            || result.contains("i64")
            || result.contains("i32"),
        "Integer variant should be present"
    );
    assert!(
        result.contains("Boolean")
            || result.contains("ConfigValueBoolean")
            || result.contains("bool"),
        "Boolean variant should be present"
    );
    assert!(
        result.contains("Number") || result.contains("ConfigValueNumber") || result.contains("f64"),
        "Number variant should be present"
    );
}
