use openapi_to_rust::analysis::{ObjectAdditionalProperties, SchemaType};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn legacy_spec() -> Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "legacy nullable container values", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "LegacyNode": {
                    "type": "object",
                    "nullable": true,
                    "required": ["name"],
                    "properties": { "name": { "type": "string" } }
                },
                "LegacyAlias": { "$ref": "#/components/schemas/LegacyNode" },
                "LegacyEnvelope": {
                    "type": "object",
                    "required": ["items"],
                    "properties": {
                        "items": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/LegacyAlias" }
                        }
                    }
                }
            }
        }
    })
}

fn modern_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "nullable container values", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "Node": {
                    "type": "object",
                    "required": ["name", "children"],
                    "properties": {
                        "name": { "type": "string" },
                        "children": {
                            "type": "array",
                            "items": {
                                "anyOf": [
                                    { "$ref": "#/components/schemas/Node" },
                                    { "type": "null" }
                                ]
                            }
                        }
                    }
                },
                "MaybeNode": {
                    "anyOf": [
                        { "$ref": "#/components/schemas/Node" },
                        { "type": "null" }
                    ]
                },
                "MaybeNodeAlias": { "$ref": "#/components/schemas/MaybeNode" },
                "NodePair": {
                    "type": "array",
                    "prefixItems": [
                        { "$ref": "#/components/schemas/MaybeNodeAlias" },
                        { "$ref": "#/components/schemas/MaybeNode" }
                    ],
                    "items": false,
                    "minItems": 2,
                    "maxItems": 2
                },
                "NodeLookup": {
                    "type": "object",
                    "additionalProperties": {
                        "$ref": "#/components/schemas/MaybeNodeAlias"
                    }
                },
                "Envelope": {
                    "type": "object",
                    "required": ["items", "nested", "pair", "lookup"],
                    "properties": {
                        "items": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/MaybeNodeAlias" }
                        },
                        "nested": {
                            "type": "array",
                            "items": {
                                "type": "array",
                                "items": { "$ref": "#/components/schemas/MaybeNode" }
                            }
                        },
                        "pair": { "$ref": "#/components/schemas/NodePair" },
                        "lookup": { "$ref": "#/components/schemas/NodeLookup" }
                    }
                }
            }
        }
    })
}

fn assert_nullable_reference(schema_type: &SchemaType, target: &str) {
    let SchemaType::Nullable { inner_type } = schema_type else {
        panic!("expected nullable container value, got {schema_type:?}");
    };
    assert!(
        matches!(inner_type.as_ref(), SchemaType::Reference { target: actual } if actual == target),
        "expected nullable reference to {target}, got {inner_type:?}"
    );
}

#[test]
fn openapi_30_nullable_reference_chains_reach_array_items() {
    let analysis = SchemaAnalyzer::new(legacy_spec())
        .expect("legacy spec should parse")
        .analyze()
        .expect("legacy spec should analyze");
    let SchemaType::Object { properties, .. } = &analysis.schemas["LegacyEnvelope"].schema_type
    else {
        panic!("LegacyEnvelope should be an object");
    };
    let SchemaType::Array { item_type } = &properties["items"].schema_type else {
        panic!("items should be an array");
    };
    assert_nullable_reference(item_type, "LegacyAlias");

    let mut generated_analysis = analysis;
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut generated_analysis)
        .expect("legacy nullable array should generate");
    assert!(
        generated.contains("pub items: Vec<Option<LegacyAlias>>"),
        "legacy nullable item reference was not wrapped:\n{generated}"
    );
}

#[test]
fn nullable_container_ir_covers_nested_arrays_tuples_maps_and_recursive_boxing() {
    let analysis = SchemaAnalyzer::new(modern_spec())
        .expect("modern spec should parse")
        .analyze()
        .expect("modern spec should analyze");

    let SchemaType::Object { properties, .. } = &analysis.schemas["Envelope"].schema_type else {
        panic!("Envelope should be an object");
    };
    let SchemaType::Array { item_type } = &properties["items"].schema_type else {
        panic!("items should be an array");
    };
    assert_nullable_reference(item_type, "MaybeNodeAlias");

    let SchemaType::Array { item_type } = &properties["nested"].schema_type else {
        panic!("nested should be an array");
    };
    let SchemaType::Array {
        item_type: nested_item,
    } = item_type.as_ref()
    else {
        panic!("nested items should themselves be arrays: {item_type:?}");
    };
    assert_nullable_reference(nested_item, "MaybeNode");

    let SchemaType::Tuple { element_types } = &analysis.schemas["NodePair"].schema_type else {
        panic!("NodePair should be an exact tuple");
    };
    assert_nullable_reference(&element_types[0], "MaybeNodeAlias");
    assert_nullable_reference(&element_types[1], "MaybeNode");

    let SchemaType::Object {
        additional_properties: ObjectAdditionalProperties::Typed { value_type },
        ..
    } = &analysis.schemas["NodeLookup"].schema_type
    else {
        panic!("NodeLookup should have typed additional properties");
    };
    assert_nullable_reference(value_type, "MaybeNodeAlias");

    let SchemaType::Object {
        properties: node_properties,
        ..
    } = &analysis.schemas["Node"].schema_type
    else {
        panic!("Node should be an object");
    };
    let SchemaType::Array { item_type } = &node_properties["children"].schema_type else {
        panic!("Node.children should be an array");
    };
    assert_nullable_reference(item_type, "Node");

    let mut generated_analysis = analysis;
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut generated_analysis)
        .expect("modern nullable containers should generate");
    let compact = generated.split_whitespace().collect::<String>();
    for expected in [
        // Vec already supplies the recursive indirection, so the nullable
        // wrapper belongs immediately around the element type.
        "pubchildren:Vec<Option<Node>>",
        "pubitems:Vec<Option<MaybeNodeAlias>>",
        "pubnested:Vec<Vec<Option<MaybeNode>>>",
        "pubtypeNodePair=(Option<MaybeNodeAlias>,Option<MaybeNode>);",
        "BTreeMap<String,Option<MaybeNodeAlias>,>",
    ] {
        assert!(
            compact.contains(expected),
            "missing generated nullable-container fragment {expected:?}:\n{generated}"
        );
    }
}

#[test]
fn generated_nullable_containers_round_trip_explicit_nulls_exactly() {
    let mut analysis = SchemaAnalyzer::new(modern_spec())
        .expect("modern spec should parse")
        .analyze()
        .expect("modern spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("modern nullable containers should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod nullable_container_roundtrip {
    use super::Envelope;

    #[test]
    fn explicit_null_elements_survive_every_container() {
        let input = serde_json::json!({
            "items": [
                null,
                {"name": "item", "children": [null, {"name": "leaf", "children": []}]}
            ],
            "nested": [[null, {"name": "nested", "children": []}]],
            "pair": [null, {"name": "pair", "children": [null]}],
            "lookup": {
                "missing": null,
                "present": {"name": "map", "children": []}
            }
        });
        let hydrated: Envelope = serde_json::from_value(input.clone()).expect("hydrate");
        let output = serde_json::to_value(hydrated).expect("serialize");
        assert_eq!(output, input);
        let stable: Envelope = serde_json::from_value(output).expect("rehydrate");
        assert_eq!(serde_json::to_value(stable).unwrap(), input);
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "nullable-container-roundtrip-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    fs::create_dir(temp.path().join("src")).expect("create scratch source directory");
    fs::write(temp.path().join("src/lib.rs"), generated).expect("write generated source");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/nullable-container-roundtrip-smoke"),
        )
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .expect("run generated round-trip test");
    assert!(
        output.status.success(),
        "generated nullable-container round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
