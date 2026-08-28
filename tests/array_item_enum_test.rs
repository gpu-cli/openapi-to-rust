#![cfg(feature = "test-helpers")]

//! Tests for inline string enums in array items (gh#33).
//!
//! Array items with `type: string` + `enum` must hoist to a named enum
//! (`{Parent}Item`) instead of collapsing to `Vec<String>`, matching the
//! existing hoisting behavior for property-level inline enums.

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn test_array_item_string_enum_is_hoisted() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Profile": {
                    "type": "object",
                    "properties": {
                        "languages": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["EN", "ES"]
                            }
                        }
                    }
                }
            }
        }
    });

    let result = test_generation("array_item_string_enum", spec).expect("Generation failed");

    assert!(
        result.contains("pub enum ProfileLanguagesItem"),
        "Should generate a named enum for array items, got:\n{result}"
    );
    assert!(
        result.contains("pub languages: Option<Vec<ProfileLanguagesItem>>"),
        "Array field should use the generated enum type, got:\n{result}"
    );
    assert!(
        !result.contains("pub languages: Option<Vec<String>>"),
        "Array field should NOT collapse to Vec<String>"
    );
}

#[test]
fn test_nullable_anyof_array_item_enum_is_hoisted() {
    // Exact repro from gh#33: anyOf [array-of-enum, null] on an inline
    // response schema.
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "repro", "version": "1.0.0"},
        "paths": {
            "/profile": {
                "get": {
                    "operationId": "getProfile",
                    "responses": {
                        "200": {
                            "description": "OK",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "languages": {
                                                "anyOf": [
                                                    {
                                                        "type": "array",
                                                        "items": {
                                                            "type": "string",
                                                            "enum": ["EN", "ES"]
                                                        }
                                                    },
                                                    {"type": "null"}
                                                ]
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let result =
        test_generation("nullable_anyof_array_item_enum", spec).expect("Generation failed");

    assert!(
        result.contains("pub enum GetProfileResponseLanguagesItem"),
        "Should generate a named enum for nullable array items, got:\n{result}"
    );
    assert!(
        result.contains("pub languages: Option<Option<Vec<GetProfileResponseLanguagesItem>>>"),
        "Optional nullable array field should preserve presence and use the generated enum type, got:\n{result}"
    );
    assert!(
        result.contains("#[serde(rename = \"EN\")]")
            && result.contains("#[serde(rename = \"ES\")]"),
        "Enum variants should rename to the spec values, got:\n{result}"
    );
}

#[test]
fn test_typeless_array_item_enum_is_hoisted() {
    // OpenAPI 3.1 allows enum without an explicit `type`; the item type is
    // inferred as string and must hoist the same way.
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Widget": {
                    "type": "object",
                    "properties": {
                        "sizes": {
                            "type": "array",
                            "items": {
                                "enum": ["S", "M", "L"]
                            }
                        }
                    }
                }
            }
        }
    });

    let result = test_generation("typeless_array_item_enum", spec).expect("Generation failed");

    assert!(
        result.contains("pub enum WidgetSizesItem"),
        "Should generate a named enum for typeless enum array items, got:\n{result}"
    );
    assert!(
        result.contains("pub sizes: Option<Vec<WidgetSizesItem>>"),
        "Array field should use the generated enum type, got:\n{result}"
    );
}

#[test]
fn test_same_named_array_item_enums_with_different_values_disambiguate() {
    // Two schemas whose array-item enums land on different names keep their
    // own variants; a recurring property name with different values must not
    // silently overwrite the first registration.
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Outer": {
                    "type": "object",
                    "properties": {
                        "tags": {
                            "type": "array",
                            "items": {"type": "string", "enum": ["RED", "BLUE"]}
                        },
                        "nested": {
                            "type": "object",
                            "properties": {
                                "tags": {
                                    "type": "array",
                                    "items": {"type": "string", "enum": ["HOT", "COLD"]}
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let result =
        test_generation("array_item_enum_disambiguation", spec).expect("Generation failed");

    for value in ["RED", "BLUE", "HOT", "COLD"] {
        assert!(
            result.contains(&format!("#[serde(rename = \"{value}\")]")),
            "Both enums should survive with all variants; missing {value}:\n{result}"
        );
    }
    assert!(
        !result.contains("Vec<String>"),
        "Neither array should collapse to Vec<String>, got:\n{result}"
    );
}

#[test]
fn test_plain_string_array_items_stay_string() {
    // No enum on the items — behavior must not change.
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Doc": {
                    "type": "object",
                    "properties": {
                        "lines": {
                            "type": "array",
                            "items": {"type": "string"}
                        }
                    }
                }
            }
        }
    });

    let result = test_generation("plain_string_array_items", spec).expect("Generation failed");

    assert!(
        result.contains("pub lines: Option<Vec<String>>"),
        "Plain string arrays should stay Vec<String>, got:\n{result}"
    );
}
