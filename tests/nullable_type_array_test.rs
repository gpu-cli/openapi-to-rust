#![cfg(feature = "test-helpers")]

//! Tests for OpenAPI 3.1 canonical nullability on *required* properties
//! (openapi-generator-dsu).
//!
//! A property that is both listed in `required` and declared nullable via the
//! 3.1 type-array form (`type: ["string", "null"]`) must generate
//! `Option<String>`. Emitting a bare `String` compiles, then fails at runtime
//! the first time the API sends `null` — the worst kind of breakage, because
//! nothing catches it until a real response arrives.
//!
//! Found by live-testing a generated client against the RunPod v2 API, where
//! `GpuType.pool` and `Pod.template` are null in production.

use openapi_to_rust::test_helpers::*;
use serde_json::json;

#[test]
fn required_type_array_null_property_is_optional() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "GpuType": {
                    "type": "object",
                    "required": ["id", "pool"],
                    "properties": {
                        "id": {"type": "string"},
                        "pool": {"type": ["string", "null"]}
                    }
                }
            }
        }
    });

    let result = test_generation("required_type_array_null", spec).expect("Generation failed");

    assert!(
        result.contains("pub pool: Option<String>"),
        "A required property typed [\"string\", \"null\"] must be Option, got:\n{result}"
    );
    assert!(
        result.contains("pub id: String") && !result.contains("pub id: Option<String>"),
        "A required non-nullable property must stay non-Option, got:\n{result}"
    );
}

/// The allOf-composed case, which merges properties through a separate code
/// path. RunPod's `Pod` is shaped exactly like this: the `required` list and
/// the nullable properties both live inside an allOf branch.
#[test]
fn required_type_array_null_through_allof_is_optional() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "ContainerConfig": {
                    "type": "object",
                    "properties": {"image": {"type": "string"}}
                },
                "Pod": {
                    "allOf": [
                        {"$ref": "#/components/schemas/ContainerConfig"},
                        {
                            "type": "object",
                            "required": ["id", "startedAt", "template"],
                            "properties": {
                                "id": {"type": "string"},
                                "startedAt": {"type": ["string", "null"], "format": "date-time"},
                                "template": {"type": ["string", "null"]}
                            }
                        }
                    ]
                }
            }
        }
    });

    let result = test_generation("required_type_array_null_allof", spec).expect("Generation failed");

    assert!(
        result.contains("pub template: Option<String>"),
        "A required nullable property merged through allOf must be Option, got:\n{result}"
    );
    assert!(
        result.contains("pub started_at: Option<"),
        "A required nullable date-time merged through allOf must be Option, got:\n{result}"
    );
}

/// The 3.0 and anyOf spellings must keep working — the fix consolidated three
/// separate checks into one helper, so all three forms are covered here to
/// stop a future refactor from dropping one again.
#[test]
fn all_three_nullability_spellings_agree() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Mixed": {
                    "type": "object",
                    "required": ["legacy", "type_array", "any_of"],
                    "properties": {
                        "legacy": {"type": "string", "nullable": true},
                        "type_array": {"type": ["string", "null"]},
                        "any_of": {
                            "anyOf": [{"type": "string"}, {"type": "null"}]
                        }
                    }
                }
            }
        }
    });

    let result = test_generation("all_nullability_spellings", spec).expect("Generation failed");

    for field in ["legacy", "type_array", "any_of"] {
        assert!(
            result.contains(&format!("pub {field}: Option<")),
            "required nullable `{field}` must be Option, got:\n{result}"
        );
    }
}
