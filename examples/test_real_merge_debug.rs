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
                println!("🔍 Found schema variants to merge:");
                println!("  Main variants: {}", main_variants.len());
                for (i, variant) in main_variants.iter().enumerate() {
                    if let Some(ref_str) = variant.get("$ref").and_then(|r| r.as_str()) {
                        println!("    {}: {}", i, ref_str);
                    }
                }
                println!("  Extension variants: {}", ext_variants.len());
                for (i, variant) in ext_variants.iter().enumerate() {
                    if let Some(ref_str) = variant.get("$ref").and_then(|r| r.as_str()) {
                        println!("    {}: {}", i, ref_str);
                    }
                }

                // Merge the variant arrays and use oneOf as the canonical key
                // First, collect main variants, but filter out any that will be replaced by extension
                let mut merged_variants = Vec::new();
                let extension_refs: Vec<String> = ext_variants
                    .iter()
                    .filter_map(|v| v.get("$ref").and_then(|r| r.as_str()))
                    .map(|s| s.to_string())
                    .collect();

                println!("  Extension refs: {:?}", extension_refs);

                // Add main variants that aren't being replaced
                for main_variant in main_variants {
                    if let Some(main_ref) = main_variant.get("$ref").and_then(|r| r.as_str()) {
                        // Check if this main variant should be replaced by an extension variant
                        let schema_name = main_ref.split('/').next_back().unwrap_or("");
                        let should_replace = extension_refs.iter().any(|ext_ref| {
                            let ext_schema_name = ext_ref.split('/').next_back().unwrap_or("");
                            // Replace specifically ResponseErrorEvent with ActualErrorEvent
                            let error_replacement = schema_name == "ResponseErrorEvent"
                                && ext_schema_name == "ActualErrorEvent";
                            let exact_match = schema_name == ext_schema_name;

                            println!(
                                "    Checking {} vs {}: error_replacement={}, exact_match={}",
                                schema_name, ext_schema_name, error_replacement, exact_match
                            );

                            error_replacement || exact_match
                        });

                        if !should_replace {
                            println!("    ✅ Keeping: {}", main_ref);
                            merged_variants.push(main_variant);
                        } else {
                            println!("    ❌ Replacing: {}", main_ref);
                        }
                    } else {
                        // Keep non-ref variants
                        merged_variants.push(main_variant);
                    }
                }

                // Add all extension variants
                for ext_variant in ext_variants {
                    if let Some(ext_ref) = ext_variant.get("$ref").and_then(|r| r.as_str()) {
                        println!("    ➕ Adding: {}", ext_ref);
                    }
                    merged_variants.push(ext_variant);
                }

                println!("  Final merged variants: {}", merged_variants.len());
                for (i, variant) in merged_variants.iter().enumerate() {
                    if let Some(ref_str) = variant.get("$ref").and_then(|r| r.as_str()) {
                        println!("    {}: {}", i, ref_str);
                    }
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
    // Exact real scenario: main spec has anyOf with ResponseErrorEvent, extension has oneOf with ActualErrorEvent
    let main_spec = json!({
        "components": {
            "schemas": {
                "ResponseStreamEvent": {
                    "anyOf": [
                        {"$ref": "#/components/schemas/ResponseCreatedEvent"},
                        {"$ref": "#/components/schemas/ResponseErrorEvent"},
                        {"$ref": "#/components/schemas/ResponseCompletedEvent"}
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

    println!("=== TESTING ERROR REPLACEMENT LOGIC ===\n");

    let result = merge_json_objects(main_spec, extension);

    println!("\n=== FINAL MERGED RESULT ===");
    println!("{}", serde_json::to_string_pretty(&result).unwrap());

    // Analyze the result
    let response_stream_event = &result["components"]["schemas"]["ResponseStreamEvent"];

    println!("\n=== ANALYSIS ===");
    if let Some(one_of) = response_stream_event.get("oneOf") {
        let variants = one_of.as_array().unwrap();
        println!("✅ Has oneOf with {} variants", variants.len());
        for (i, variant) in variants.iter().enumerate() {
            if let Some(ref_str) = variant.get("$ref").and_then(|r| r.as_str()) {
                let schema_name = ref_str.split('/').next_back().unwrap_or("");
                println!("  {}: {} ({})", i, ref_str, schema_name);
            }
        }

        let has_error_event = variants.iter().any(|v| {
            v.get("$ref")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .contains("ResponseErrorEvent")
        });
        let has_actual_error_event = variants.iter().any(|v| {
            v.get("$ref")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .contains("ActualErrorEvent")
        });

        if has_error_event && has_actual_error_event {
            println!("❌ PROBLEM: Both ResponseErrorEvent and ActualErrorEvent present!");
        } else if has_actual_error_event && !has_error_event {
            println!("✅ SUCCESS: ResponseErrorEvent replaced with ActualErrorEvent");
        } else if has_error_event && !has_actual_error_event {
            println!("❌ PROBLEM: ActualErrorEvent not added, ResponseErrorEvent still there");
        } else {
            println!("❌ PROBLEM: No error events found");
        }
    } else {
        println!("❌ Missing oneOf property");
    }
}
