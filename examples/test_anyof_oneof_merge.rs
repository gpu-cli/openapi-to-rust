use serde_json::{Value, json};

// Copy the merge function from analysis.rs for testing
fn merge_json_objects(main: Value, extension: Value) -> Value {
    match (main, extension) {
        // Both objects - merge properties
        (Value::Object(mut main_obj), Value::Object(ext_obj)) => {
            // Special handling for schema objects with oneOf/anyOf variants
            if let (Some(main_variants), Some(ext_variants)) = (
                extract_schema_variants(&Value::Object(main_obj.clone())),
                extract_schema_variants(&Value::Object(ext_obj.clone())),
            ) {
                // Merge the variant arrays and use oneOf as the canonical key
                // First, collect main variants, but filter out any that will be replaced by extension
                let mut merged_variants = Vec::new();
                let extension_refs: Vec<String> = ext_variants
                    .iter()
                    .filter_map(|v| v.get("$ref").and_then(|r| r.as_str()))
                    .map(|s| s.to_string())
                    .collect();

                // Add main variants that aren't being replaced
                for main_variant in main_variants {
                    if let Some(main_ref) = main_variant.get("$ref").and_then(|r| r.as_str()) {
                        // Check if this main variant should be replaced by an extension variant
                        let schema_name = main_ref.split('/').next_back().unwrap_or("");
                        let should_replace = extension_refs.iter().any(|ext_ref| {
                            let ext_schema_name = ext_ref.split('/').next_back().unwrap_or("");
                            // Replace if the extension has an error event and main has an error event
                            (schema_name.contains("Error") && ext_schema_name.contains("Error")) ||
                            // Or exact match
                            schema_name == ext_schema_name
                        });

                        if !should_replace {
                            merged_variants.push(main_variant);
                        }
                    } else {
                        // Keep non-ref variants
                        merged_variants.push(main_variant);
                    }
                }

                // Add all extension variants
                for ext_variant in ext_variants {
                    merged_variants.push(ext_variant);
                }

                // Remove old oneOf/anyOf keys and add the merged oneOf
                main_obj.remove("oneOf");
                main_obj.remove("anyOf");
                main_obj.insert("oneOf".to_string(), Value::Array(merged_variants));

                // Merge other properties normally
                for (key, ext_value) in ext_obj {
                    if key != "oneOf" && key != "anyOf" {
                        match main_obj.get(&key) {
                            Some(main_value) => {
                                let merged_value =
                                    merge_json_objects(main_value.clone(), ext_value);
                                main_obj.insert(key, merged_value);
                            }
                            None => {
                                main_obj.insert(key, ext_value);
                            }
                        }
                    }
                }

                return Value::Object(main_obj);
            }

            // Normal object merging
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

/// Extract schema variants from oneOf or anyOf properties
fn extract_schema_variants(obj: &Value) -> Option<Vec<Value>> {
    if let Value::Object(map) = obj {
        if let Some(Value::Array(variants)) = map.get("oneOf") {
            return Some(variants.clone());
        }
        if let Some(Value::Array(variants)) = map.get("anyOf") {
            return Some(variants.clone());
        }
    }
    None
}

fn main() {
    // Real scenario: main spec has anyOf with ResponseErrorEvent, extension has oneOf with ActualErrorEvent
    let main_spec = json!({
        "components": {
            "schemas": {
                "ResponseStreamEvent": {
                    "anyOf": [
                        {"$ref": "#/components/schemas/ResponseAudioDeltaEvent"},
                        {"$ref": "#/components/schemas/ResponseCompletedEvent"},
                        {"$ref": "#/components/schemas/ResponseErrorEvent"},
                        {"$ref": "#/components/schemas/ResponseTextDeltaEvent"}
                    ]
                }
            }
        }
    });

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

    println!("=== MAIN SPEC (anyOf) ===");
    println!("{}", serde_json::to_string_pretty(&main_spec).unwrap());

    println!("\n=== EXTENSION (oneOf) ===");
    println!("{}", serde_json::to_string_pretty(&extension).unwrap());

    let result = merge_json_objects(main_spec, extension);

    println!("\n=== MERGED RESULT ===");
    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    // Analyze the result
    let response_stream_event = &result["components"]["schemas"]["ResponseStreamEvent"];

    println!("\n=== ANALYSIS ===");
    if let Some(one_of) = response_stream_event.get("oneOf") {
        println!(
            "✅ Has oneOf with {} variants",
            one_of.as_array().unwrap().len()
        );
        println!(
            "   Variants: {:?}",
            one_of
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.get("$ref").unwrap().as_str().unwrap())
                .collect::<Vec<_>>()
        );
    } else {
        println!("❌ Missing oneOf property");
    }

    if let Some(any_of) = response_stream_event.get("anyOf") {
        println!(
            "❌ Still has anyOf (should have been removed): {} variants",
            any_of.as_array().unwrap().len()
        );
    } else {
        println!("✅ anyOf property correctly removed");
    }
}
