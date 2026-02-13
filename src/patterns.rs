use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum AnyOfSemantics {
    /// Nullable type pattern: [Type, null]
    Nullable,
    /// Union type pattern: multiple complex types
    Union,
    /// Flexible union: mixed refs and primitives
    FlexibleUnion,
}

#[derive(Debug, Clone)]
pub struct DiscriminatedUnionInfo {
    pub field: String,
    pub mappings: std::collections::HashMap<String, String>,
}

pub fn analyze_anyof_semantics(schema: &Value) -> Option<AnyOfSemantics> {
    let variants = schema.get("anyOf")?.as_array()?;

    // Pattern 1: [Type, null] = nullable type
    if variants.len() == 2 {
        let has_null = variants.iter().any(is_null_type);
        let has_type = variants.iter().any(|v| !is_null_type(v));

        if has_null && has_type {
            return Some(AnyOfSemantics::Nullable);
        }
    }

    // Pattern 2: Multiple complex types = union
    if variants.iter().all(is_complex_type) {
        return Some(AnyOfSemantics::Union);
    }

    // Pattern 3: Mixed refs and primitives = flexible union
    Some(AnyOfSemantics::FlexibleUnion)
}

pub fn is_null_type(schema: &Value) -> bool {
    schema.get("type").and_then(|t| t.as_str()) == Some("null")
}

pub fn is_complex_type(schema: &Value) -> bool {
    // Consider refs and objects as complex types
    schema.get("$ref").is_some()
        || schema.get("type").and_then(|t| t.as_str()) == Some("object")
        || schema.get("properties").is_some()
}

pub fn detect_discriminated_union(schema: &Value) -> Option<DiscriminatedUnionInfo> {
    // Check for explicit discriminator field
    if let Some(discriminator) = schema.get("discriminator") {
        return parse_explicit_discriminator(discriminator, schema);
    }

    // Auto-detect from oneOf/anyOf patterns
    if let Some(variants) = schema.get("oneOf").or_else(|| schema.get("anyOf")) {
        return detect_implicit_discriminator(variants);
    }

    None
}

fn parse_explicit_discriminator(
    discriminator: &Value,
    schema: &Value,
) -> Option<DiscriminatedUnionInfo> {
    let field = discriminator.get("propertyName")?.as_str()?.to_string();

    // Extract mappings from discriminator.mapping if present
    let mut mappings = std::collections::HashMap::new();

    if let Some(mapping) = discriminator.get("mapping") {
        if let Some(mapping_obj) = mapping.as_object() {
            for (key, value) in mapping_obj {
                if let Some(value_str) = value.as_str() {
                    mappings.insert(key.clone(), value_str.to_string());
                }
            }
        }
    } else {
        // No explicit mapping - need to extract from variant schemas
        if let Some(variants) = schema.get("oneOf").or_else(|| schema.get("anyOf")) {
            if let Some(variants_array) = variants.as_array() {
                mappings = extract_variant_discriminator_mappings(variants_array, &field)?;
            }
        }
    }

    Some(DiscriminatedUnionInfo { field, mappings })
}

fn extract_variant_discriminator_mappings(
    variants: &[Value],
    _discriminator_field: &str,
) -> Option<std::collections::HashMap<String, String>> {
    let mut mappings = std::collections::HashMap::new();

    for variant in variants {
        if let Some(ref_str) = variant.get("$ref").and_then(|r| r.as_str()) {
            if let Some(schema_name) = extract_schema_name_from_ref(ref_str) {
                // For now, we can't resolve the actual schema here since we don't have access to all schemas
                // The actual discriminator value extraction will be handled in analysis.rs
                // We'll create a placeholder mapping using the schema name
                mappings.insert(schema_name.clone(), format!("schema:{schema_name}"));
            }
        }
    }

    if mappings.is_empty() {
        None
    } else {
        Some(mappings)
    }
}

fn extract_schema_name_from_ref(ref_str: &str) -> Option<String> {
    ref_str.split('/').next_back().map(|s| s.to_string())
}

fn detect_implicit_discriminator(variants: &Value) -> Option<DiscriminatedUnionInfo> {
    let variant_refs = extract_variant_refs(variants)?;

    // Check if all variants have a common field that could be a discriminator
    let common_discriminator = find_common_discriminator_field(&variant_refs)?;

    // TODO: Verify each variant has a unique value for the discriminator
    // This would require resolving the schema references

    Some(DiscriminatedUnionInfo {
        field: common_discriminator,
        mappings: std::collections::HashMap::new(),
    })
}

fn extract_variant_refs(variants: &Value) -> Option<Vec<String>> {
    let variants_array = variants.as_array()?;

    let refs: Vec<String> = variants_array
        .iter()
        .filter_map(|v| v.get("$ref").and_then(|r| r.as_str()))
        .map(|s| s.to_string())
        .collect();

    if refs.len() == variants_array.len() && refs.len() > 1 {
        Some(refs)
    } else {
        None
    }
}

fn find_common_discriminator_field(_variant_refs: &[String]) -> Option<String> {
    // For now, assume "type" is the common discriminator field
    // In a full implementation, this would resolve each ref and check
    // for common fields across all variants
    Some("type".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_nullable_pattern_detection() {
        let nullable_schema = json!({
            "anyOf": [
                {"$ref": "#/components/schemas/Error"},
                {"type": "null"}
            ]
        });

        assert_eq!(
            analyze_anyof_semantics(&nullable_schema),
            Some(AnyOfSemantics::Nullable)
        );
    }

    #[test]
    fn test_union_pattern_detection() {
        let union_schema = json!({
            "anyOf": [
                {"$ref": "#/components/schemas/TextContent"},
                {"$ref": "#/components/schemas/ImageContent"}
            ]
        });

        assert_eq!(
            analyze_anyof_semantics(&union_schema),
            Some(AnyOfSemantics::Union)
        );
    }

    #[test]
    fn test_discriminated_union_detection() {
        let schema = json!({
            "oneOf": [
                {"$ref": "#/components/schemas/ResponseCreatedEvent"},
                {"$ref": "#/components/schemas/ResponseTextDeltaEvent"}
            ],
            "discriminator": {
                "propertyName": "type"
            }
        });

        let result = detect_discriminated_union(&schema);
        assert!(result.is_some());

        let info = result.unwrap();
        assert_eq!(info.field, "type");
    }
}
