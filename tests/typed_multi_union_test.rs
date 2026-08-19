#![cfg(feature = "test-helpers")]

//! A JSON-Schema-2020-12-style `type: [X, Y]` array with two *non-null*
//! scalar types must generate an untagged enum covering both branches,
//! not silently collapse to whichever type is listed first.

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn two_scalar_type_array_property_becomes_untagged_enum() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Widget": {
                    "type": "object",
                    "properties": {
                        "id_or_code": {"type": ["integer", "string"]}
                    }
                }
            }
        }
    });

    let result =
        test_generation("two_scalar_type_array_property", spec).expect("Generation failed");

    assert!(
        result.contains("pub id_or_code: Option<WidgetIdOrCode>"),
        "a two-scalar type array property must reference a named union enum, got:\n{result}"
    );
    assert!(
        result.contains("enum WidgetIdOrCode"),
        "the named union enum must be generated, got:\n{result}"
    );
    assert!(
        result.contains("Integer(i64)") && result.contains("String(String)"),
        "the union enum must cover both declared types, got:\n{result}"
    );
    assert!(
        result.contains("#[serde(untagged)]"),
        "the union enum must be untagged so either wire shape deserializes, got:\n{result}"
    );
}

#[test]
fn two_scalar_type_array_order_is_preserved_not_first_wins() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Widget": {
                    "type": "object",
                    "properties": {
                        "id_or_code": {"type": ["string", "integer"]}
                    }
                }
            }
        }
    });

    let result = test_generation("two_scalar_type_array_order", spec).expect("Generation failed");

    let enum_start = result
        .find("enum WidgetIdOrCode")
        .expect("union enum must be generated");
    let string_variant = result[enum_start..].find("String(String)").unwrap();
    let integer_variant = result[enum_start..].find("Integer(i64)").unwrap();
    assert!(
        string_variant < integer_variant,
        "variant order must follow the declared type order, got:\n{result}"
    );
}

#[test]
fn top_level_two_scalar_type_array_becomes_untagged_enum() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "IdOrCode": {"type": ["integer", "string"]}
            }
        }
    });

    let result =
        test_generation("top_level_two_scalar_type_array", spec).expect("Generation failed");

    assert!(
        result.contains("enum IdOrCode")
            && result.contains("Integer(i64)")
            && result.contains("String(String)"),
        "a top-level two-scalar type array schema must become an untagged enum, got:\n{result}"
    );
}

/// The 3.1 nullable shorthand (`[X, "null"]`) must keep collapsing to
/// `Option<X>` — only genuine multi-scalar unions get an enum.
#[test]
fn nullable_shorthand_still_collapses_to_option() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Widget": {
                    "type": "object",
                    "properties": {
                        "maybe_name": {"type": ["string", "null"]}
                    }
                }
            }
        }
    });

    let result = test_generation("nullable_shorthand_collapses", spec).expect("Generation failed");

    assert!(
        result.contains("pub maybe_name: Option<String>"),
        "the nullable shorthand must stay a plain Option<String>, got:\n{result}"
    );
    assert!(
        !result.contains("enum WidgetMaybeName"),
        "the nullable shorthand must not synthesize a union enum, got:\n{result}"
    );
}

/// A multi-type union member that's `"array"` shares its `items` schema
/// with the other members (`TypedMulti` carries one `SchemaDetails` for
/// the whole `type: [...]` list). The array variant must keep that item
/// type instead of collapsing to `Vec<serde_json::Value>`.
#[test]
fn array_member_of_type_array_union_keeps_item_type() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Widget": {
                    "type": "object",
                    "properties": {
                        "tags_or_tag": {
                            "type": ["array", "string"],
                            "items": {"type": "string"}
                        }
                    }
                }
            }
        }
    });

    let result =
        test_generation("array_member_of_type_array_union", spec).expect("Generation failed");

    assert!(
        result.contains("pub type WidgetTagsOrTagArray = Vec<String>"),
        "the array variant must keep its declared item type, got:\n{result}"
    );
    assert!(
        !result.contains("Vec<serde_json::Value>"),
        "the array variant must not degrade to a generic JSON array, got:\n{result}"
    );
}
