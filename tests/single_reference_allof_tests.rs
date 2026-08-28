#![cfg(feature = "test-helpers")]

//! Tests for single-reference allOf patterns
//!
//! These tests ensure that allOf patterns with a single reference
//! resolve to direct type references instead of unnecessary compositions.

use openapi_to_rust::test_helpers::*;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn generate(spec: Value, module_name: &str) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("schema analyzer");
    let mut analysis = analyzer.analyze().expect("schema analysis");
    CodeGenerator::new(GeneratorConfig {
        module_name: module_name.to_string(),
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("code generation")
}

fn struct_body<'a>(generated: &'a str, name: &str) -> &'a str {
    generated
        .split(&format!("pub struct {name} {{"))
        .nth(1)
        .and_then(|tail| tail.split("\n}").next())
        .unwrap_or_else(|| panic!("missing struct `{name}`:\n{generated}"))
}

#[test]
fn test_single_reference_allof_resolves_directly() {
    // Test that allOf with single reference creates direct type reference, not composition
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Message": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "usage": {
                            "allOf": [{"$ref": "#/components/schemas/Usage"}],
                            "description": "Usage information for this message"
                        }
                    },
                    "required": ["id", "usage"]
                },
                "Usage": {
                    "type": "object",
                    "properties": {
                        "input_tokens": {"type": "integer"},
                        "output_tokens": {"type": "integer"},
                        "total_tokens": {"type": "integer"}
                    },
                    "required": ["input_tokens", "output_tokens", "total_tokens"]
                }
            }
        }
    });

    let result = test_generation("single_ref_allof_test", spec).expect("Generation failed");

    // Verify both Usage and Message structs are generated
    assert!(
        result.contains("pub struct Usage"),
        "Usage should be generated as a struct"
    );
    assert!(
        result.contains("pub struct Message"),
        "Message should be generated as a struct"
    );

    // Verify Message has usage field of type Usage (not serde_json::Value)
    assert!(
        result.contains("pub usage: Usage"),
        "Message should have usage field of type Usage"
    );
    assert!(
        !result.contains("serde_json::Value"),
        "Generated types should not contain serde_json::Value for usage field"
    );

    // Verify no unnecessary composition wrapper is generated
    assert!(
        !result.contains("MessageUsage") && !result.contains("UsageWrapper"),
        "Should not generate unnecessary wrapper types for single-reference allOf"
    );
}

#[test]
fn test_complex_usage_structure() {
    // Test a more complex usage structure similar to Anthropic's actual usage schema
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "Message": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "usage": {
                            "allOf": [{"$ref": "#/components/schemas/Usage"}],
                            "description": "Billing and rate-limit usage"
                        }
                    },
                    "required": ["id", "usage"]
                },
                "Usage": {
                    "type": "object",
                    "properties": {
                        "input_tokens": {"type": "integer"},
                        "output_tokens": {"type": "integer"},
                        "cache_creation_input_tokens": {
                            "type": "integer",
                            "description": "Optional cache tokens"
                        },
                        "cache_read_input_tokens": {
                            "type": "integer",
                            "description": "Optional cache read tokens"
                        }
                    },
                    "required": ["input_tokens", "output_tokens"]
                }
            }
        }
    });

    let result = test_generation("complex_usage_test", spec).expect("Generation failed");

    // Verify the complex Usage struct is properly generated
    // Find the Usage struct in the generated code
    let usage_start = result
        .find("pub struct Usage")
        .expect("Usage struct should be generated");
    let usage_end = result[usage_start..].find("}").unwrap() + usage_start;
    let usage_section = &result[usage_start..=usage_end];

    assert!(
        usage_section.contains("input_tokens")
            && usage_section.contains("output_tokens")
            && usage_section.contains("cache_creation_input_tokens")
            && usage_section.contains("cache_read_input_tokens"),
        "Usage should have all expected fields"
    );

    // Verify optional fields are Option<T>
    assert!(
        usage_section.contains("Option<i64>") || usage_section.contains("Option<i32>"),
        "Optional cache fields should be Option<T>"
    );
}

#[test]
fn test_mixed_allof_composition_vs_single_reference() {
    // Test that true composition (multiple schemas) vs single reference are handled differently
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "components": {
            "schemas": {
                "MessageWithUsage": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "usage": {
                            "allOf": [{"$ref": "#/components/schemas/Usage"}],
                            "description": "Single reference - should be direct"
                        }
                    },
                    "required": ["id", "usage"]
                },
                "ExtendedMessage": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseMessage"},
                        {
                            "type": "object",
                            "properties": {
                                "timestamp": {"type": "string"}
                            },
                            "required": ["timestamp"]
                        }
                    ]
                },
                "BaseMessage": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "content": {"type": "string"}
                    },
                    "required": ["id", "content"]
                },
                "Usage": {
                    "type": "object",
                    "properties": {
                        "input_tokens": {"type": "integer"},
                        "output_tokens": {"type": "integer"}
                    },
                    "required": ["input_tokens", "output_tokens"]
                }
            }
        }
    });

    let result = test_generation("mixed_allof_test", spec).expect("Generation failed");

    // Single reference allOf should use direct type reference
    // Find the MessageWithUsage struct
    let message_start = result
        .find("pub struct MessageWithUsage")
        .expect("MessageWithUsage should be generated");
    let message_end = result[message_start..].find("}").unwrap() + message_start;
    let message_section = &result[message_start..=message_end];

    assert!(
        message_section.contains("usage") && message_section.contains(": Usage"),
        "Single reference allOf should create direct Usage field reference"
    );
    assert!(
        !message_section.contains("serde_json::Value"),
        "Single reference allOf should not fall back to serde_json::Value"
    );

    // Multi-schema allOf should create flattened struct (ExtendedMessage)
    let extended_start = result
        .find("pub struct ExtendedMessage")
        .expect("ExtendedMessage should be generated");
    let extended_end = result[extended_start..].find("}").unwrap() + extended_start;
    let extended_section = &result[extended_start..=extended_end];

    assert!(
        extended_section.contains("id")
            && extended_section.contains("content")
            && extended_section.contains("timestamp"),
        "Multi-schema allOf should flatten all properties into one struct"
    );
}

#[test]
fn reference_with_annotation_sibling_preserves_recursive_model_reference() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {"title": "recursive filter", "version": "1.0"},
        "components": { "schemas": {
            "Expression": {
                "type": "object",
                "properties": {
                    "not": { "allOf": [
                        { "$ref": "#/components/schemas/Expression" },
                        { "description": "Negate this expression" }
                    ]}
                }
            }
        }}
    });

    let result = test_generation("recursive_annotated_allof", spec).expect("Generation failed");
    let expression = result
        .split("pub struct Expression")
        .nth(1)
        .expect("Expression model")
        .split('}')
        .next()
        .unwrap();
    assert!(expression.contains("Expression"), "{expression}");
    assert!(!expression.contains("serde_json::Value"), "{expression}");
}

#[test]
fn transitive_alias_extension_preserves_inherited_required_fields() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "transitive alias", "version": "1.0"},
        "components": { "schemas": {
            "Base": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" }
                },
                "required": ["id"]
            },
            "Alias": {
                "allOf": [
                    { "$ref": "#/components/schemas/Base" },
                    { "description": "single-reference alias wrapper" }
                ]
            },
            "Extended": {
                "allOf": [
                    { "$ref": "#/components/schemas/Alias" },
                    {
                        "type": "object",
                        "properties": {
                            "enabled": { "type": "boolean" }
                        },
                        "required": ["enabled"]
                    }
                ]
            },
            "Holder": {
                "type": "object",
                "properties": {
                    "extended": {
                        "allOf": [{ "$ref": "#/components/schemas/Extended" }]
                    }
                },
                "required": ["extended"]
            }
        }}
    });

    let generated = generate(spec, "transitive_alias_extension");
    let extended = struct_body(&generated, "Extended");
    let holder = struct_body(&generated, "Holder");

    assert!(extended.contains("pub id: String"), "{generated}");
    assert!(!extended.contains("pub id: Option<"), "{generated}");
    assert!(extended.contains("pub enabled: bool"), "{generated}");
    assert!(!extended.contains("pub enabled: Option<"), "{generated}");
    assert!(holder.contains("pub extended: Extended"), "{generated}");
    assert!(!holder.contains("serde_json::Value"), "{generated}");
}

#[test]
fn three_hop_single_reference_alias_chain_stays_typed() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "three hop alias", "version": "1.0"},
        "components": { "schemas": {
            "Base": {
                "type": "object",
                "properties": {
                    "code": { "type": "string" }
                },
                "required": ["code"]
            },
            "AliasOne": {
                "allOf": [{ "$ref": "#/components/schemas/Base" }]
            },
            "AliasTwo": {
                "allOf": [
                    { "$ref": "#/components/schemas/AliasOne" },
                    { "description": "hop two" }
                ]
            },
            "AliasThree": {
                "allOf": [{ "$ref": "#/components/schemas/AliasTwo" }]
            },
            "Extended": {
                "allOf": [
                    { "$ref": "#/components/schemas/AliasThree" },
                    {
                        "type": "object",
                        "properties": {
                            "enabled": { "type": "boolean" }
                        },
                        "required": ["enabled"]
                    }
                ]
            },
            "Holder": {
                "type": "object",
                "properties": {
                    "leaf": {
                        "allOf": [{ "$ref": "#/components/schemas/Extended" }]
                    }
                },
                "required": ["leaf"]
            }
        }}
    });

    let generated = generate(spec, "three_hop_alias_chain");
    let extended = struct_body(&generated, "Extended");
    let holder = struct_body(&generated, "Holder");

    assert!(extended.contains("pub code: String"), "{generated}");
    assert!(!extended.contains("pub code: Option<"), "{generated}");
    assert!(extended.contains("pub enabled: bool"), "{generated}");
    assert!(!extended.contains("pub enabled: Option<"), "{generated}");
    assert!(holder.contains("pub leaf: Extended"), "{generated}");
    assert!(!holder.contains("Option<"), "{generated}");
    assert!(!holder.contains("serde_json::Value"), "{generated}");
    assert!(!generated.contains("pub struct HolderLeaf"), "{generated}");
}

#[test]
fn self_referential_single_reference_allof_terminates_and_keeps_outer_field() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {"title": "self cycle", "version": "1.0"},
        "components": { "schemas": {
            "SelfAlias": {
                "allOf": [{ "$ref": "#/components/schemas/SelfAlias" }]
            },
            "Extended": {
                "allOf": [
                    { "$ref": "#/components/schemas/SelfAlias" },
                    {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" }
                        },
                        "required": ["id"]
                    }
                ]
            },
            "Holder": {
                "type": "object",
                "properties": {
                    "root": {
                        "allOf": [{ "$ref": "#/components/schemas/Extended" }]
                    }
                },
                "required": ["root"]
            }
        }}
    });

    let generated = generate(spec, "self_cycle_single_reference_allof");
    let extended = struct_body(&generated, "Extended");
    let holder = struct_body(&generated, "Holder");

    assert!(extended.contains("pub id: String"), "{generated}");
    assert!(!extended.contains("pub id: Option<"), "{generated}");
    assert!(holder.contains("pub root: Extended"), "{generated}");
}

#[test]
fn two_node_single_reference_allof_cycle_terminates_and_keeps_local_fields() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {"title": "two node cycle", "version": "1.0"},
        "components": { "schemas": {
            "AliasA": {
                "allOf": [{ "$ref": "#/components/schemas/AliasB" }]
            },
            "AliasB": {
                "allOf": [{ "$ref": "#/components/schemas/AliasA" }]
            },
            "Extended": {
                "allOf": [
                    { "$ref": "#/components/schemas/AliasA" },
                    {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        },
                        "required": ["name"]
                    }
                ]
            },
            "Holder": {
                "type": "object",
                "properties": {
                    "left": {
                        "allOf": [{ "$ref": "#/components/schemas/Extended" }]
                    }
                },
                "required": ["left"]
            }
        }}
    });

    let generated = generate(spec, "two_node_cycle_single_reference_allof");
    let extended = struct_body(&generated, "Extended");
    let holder = struct_body(&generated, "Holder");

    assert!(extended.contains("pub name: String"), "{generated}");
    assert!(!extended.contains("pub name: Option<"), "{generated}");
    assert!(holder.contains("pub left: Extended"), "{generated}");
}

#[test]
fn reverse_component_order_still_resolves_transitive_single_reference_allof() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "reverse order", "version": "1.0"},
        "components": { "schemas": {
            "XExtended": {
                "allOf": [
                    { "$ref": "#/components/schemas/YAlias" },
                    {
                        "type": "object",
                        "properties": {
                            "extra": { "type": "string" }
                        },
                        "required": ["extra"]
                    }
                ]
            },
            "YAlias": {
                "allOf": [
                    { "$ref": "#/components/schemas/ZBase" },
                    { "description": "declared after dependent" }
                ]
            },
            "ZBase": {
                "type": "object",
                "properties": {
                    "base_id": { "type": "string" }
                },
                "required": ["base_id"]
            },
            "Holder": {
                "type": "object",
                "properties": {
                    "item": {
                        "allOf": [{ "$ref": "#/components/schemas/XExtended" }]
                    }
                },
                "required": ["item"]
            }
        }}
    });

    let generated = generate(spec, "reverse_component_order_alias_chain");
    let extended = struct_body(&generated, "XExtended");
    let holder = struct_body(&generated, "Holder");

    assert!(extended.contains("pub base_id: String"), "{generated}");
    assert!(!extended.contains("pub base_id: Option<"), "{generated}");
    assert!(extended.contains("pub extra: String"), "{generated}");
    assert!(!extended.contains("pub extra: Option<"), "{generated}");
    assert!(holder.contains("pub item: XExtended"), "{generated}");
    assert!(!generated.contains("serde_json::Value"), "{generated}");
}
