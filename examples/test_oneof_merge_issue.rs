use serde_json::{Value, json};

// This function shows the current behavior of merge_json_objects
fn merge_json_objects(main: Value, extension: Value) -> Value {
    match (main, extension) {
        // Both objects - merge properties
        (Value::Object(mut main_obj), Value::Object(ext_obj)) => {
            for (key, ext_value) in ext_obj {
                match main_obj.get(&key) {
                    Some(main_value) => {
                        // Key exists in both - recursively merge
                        let merged_value = merge_json_objects(main_value.clone(), ext_value);
                        main_obj.insert(key, merged_value);
                    }
                    None => {
                        // Key only in extension - add it
                        main_obj.insert(key, ext_value);
                    }
                }
            }
            Value::Object(main_obj)
        }

        // Both arrays - concatenate
        (Value::Array(mut main_arr), Value::Array(ext_arr)) => {
            main_arr.extend(ext_arr);
            Value::Array(main_arr)
        }

        // Extension overrides main for all other cases
        (_, extension) => extension,
    }
}

fn main() {
    // Simulating the main OpenAPI spec with ResponseStreamEvent that has multiple variants
    let main_spec = json!({
        "components": {
            "schemas": {
                "ResponseStreamEvent": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/MessageDelta"},
                        {"$ref": "#/components/schemas/MessageComplete"},
                        {"$ref": "#/components/schemas/ToolCall"}
                    ]
                }
            }
        }
    });

    // Simulating the extension with ResponseStreamEvent that adds one more variant
    let extension = json!({
        "components": {
            "schemas": {
                "ResponseStreamEvent": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/ActualErrorEvent"}
                    ]
                }
            }
        }
    });

    println!("=== MAIN SPEC ===");
    println!("{}", serde_json::to_string_pretty(&main_spec).unwrap());

    println!("\n=== EXTENSION ===");
    println!("{}", serde_json::to_string_pretty(&extension).unwrap());

    let result = merge_json_objects(main_spec, extension);

    println!("\n=== MERGED RESULT (CURRENT BEHAVIOR - BROKEN) ===");
    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    // Show what's wrong
    let response_stream_event = &result["components"]["schemas"]["ResponseStreamEvent"];
    let one_of = &response_stream_event["oneOf"];

    println!("\n=== ANALYSIS ===");
    println!(
        "ResponseStreamEvent oneOf variants: {}",
        one_of.as_array().unwrap().len()
    );
    println!("Expected: 4 variants (3 from main + 1 from extension)");
    println!(
        "Actual: {} variants (only extension survived)",
        one_of.as_array().unwrap().len()
    );

    if one_of.as_array().unwrap().len() == 1 {
        println!("❌ BUG CONFIRMED: Extension completely replaced main spec oneOf array");
    } else {
        println!("✅ Working correctly");
    }
}
