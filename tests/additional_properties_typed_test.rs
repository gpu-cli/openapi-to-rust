//! Q2.3: typed `BTreeMap<String, T>` from
//! `additionalProperties: <schema>` (default on; opt out via
//! `[generator.types.shape] additional_properties_typed = false`).

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
    type_mapping::TypeShapeConfig,
};
use serde_json::json;
use std::process::Command;

fn ap_spec(value_schema: serde_json::Value) -> serde_json::Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "additionalProperties": value_schema
                }
            }
        }
    })
}

fn generate(spec: serde_json::Value, mapper: TypeMapper) -> String {
    let mut analyzer = SchemaAnalyzer::with_type_mapper(spec, mapper).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        ..Default::default()
    };
    CodeGenerator::new(cfg)
        .generate(&mut analysis)
        .expect("generate")
}

fn analyze_error(spec: serde_json::Value) -> String {
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::new(TypeMappingConfig::default()))
            .expect("analyzer");
    match analyzer.analyze() {
        Ok(_) => panic!("schema analysis unexpectedly succeeded"),
        Err(error) => error.to_string(),
    }
}

fn struct_header<'a>(code: &'a str, name: &str) -> &'a str {
    let marker = format!("pub struct {name}");
    let struct_start = code
        .find(&marker)
        .unwrap_or_else(|| panic!("generated code did not contain `{marker}`:\n{code}"));
    let derive_start = code[..struct_start]
        .rfind("#[derive(")
        .unwrap_or_else(|| panic!("generated `{name}` did not have a derive attribute:\n{code}"));
    &code[derive_start..struct_start]
}

fn struct_source<'a>(code: &'a str, name: &str) -> &'a str {
    let marker = format!("pub struct {name}");
    let start = code
        .find(&marker)
        .unwrap_or_else(|| panic!("generated code did not contain `{marker}`:\n{code}"));
    let end = code[start..]
        .find("\n}")
        .map(|offset| start + offset + 2)
        .unwrap_or_else(|| panic!("generated `{name}` was not closed:\n{code}"));
    &code[start..end]
}

#[test]
fn ap_string_schema_default_emits_typed_btreemap() {
    let code = generate(
        ap_spec(json!({ "type": "string" })),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, String>"),
        "additionalProperties: <string schema> should produce BTreeMap<String, String>. Code:\n{code}"
    );
}

#[test]
fn ap_integer_schema_default_emits_typed_btreemap() {
    let code = generate(
        ap_spec(json!({ "type": "integer", "format": "int32" })),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, i32>"),
        "additionalProperties with int32 should produce BTreeMap<String, i32>. Code:\n{code}"
    );
}

#[test]
fn ap_typed_default_picks_up_format_typed_scalars() {
    // The value-type analysis should respect TypeMapper format
    // strategies, so additionalProperties: { format: uuid } yields
    // BTreeMap<String, uuid::Uuid>.
    let code = generate(
        ap_spec(json!({ "type": "string", "format": "uuid" })),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, uuid::Uuid>"),
        "additionalProperties with format: uuid should produce BTreeMap<String, uuid::Uuid>. Code:\n{code}"
    );
}

#[test]
fn ap_boolean_true_remains_untyped() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "additionalProperties": true
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains(
            "pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>"
        ),
        "additionalProperties: true should still produce BTreeMap<String, serde_json::Value>. Code:\n{code}"
    );
}

#[test]
fn ap_boolean_false_emits_no_field() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        !code.contains("pub additional_properties:"),
        "additionalProperties: false must not emit a field. Code:\n{code}"
    );
}

#[test]
fn ap_typed_off_falls_back_to_untyped() {
    let mut cfg = TypeMappingConfig::default();
    cfg.shape = Some(TypeShapeConfig {
        additional_properties_typed: Some(false),
        ..Default::default()
    });
    let code = generate(ap_spec(json!({ "type": "string" })), TypeMapper::new(cfg));
    assert!(
        code.contains(
            "pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>"
        ),
        "additional_properties_typed = false should degrade to serde_json::Value. Code:\n{code}"
    );
}

#[test]
fn ap_schema_ref_emits_btreemap_of_named_type() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Item": {
                    "type": "object",
                    "required": ["name"],
                    "properties": { "name": { "type": "string" } }
                },
                "Bag": {
                    "type": "object",
                    "additionalProperties": { "$ref": "#/components/schemas/Item" }
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, Item>"),
        "additionalProperties: $ref should produce BTreeMap<String, NamedType>. Code:\n{code}"
    );
}

#[test]
fn required_undeclared_member_with_omitted_ap_is_a_value_and_keeps_extras() {
    let spec = json!({
        "openapi": "3.0.3",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "properties": {
                        "label": { "type": "string" }
                    },
                    "required": ["payload"]
                }
            }
        }
    });

    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains("pub payload: serde_json::Value"),
        "an undeclared required member with omitted additionalProperties must be retained as a required JSON value. Code:\n{code}"
    );
    assert!(
        code.contains(
            "pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>"
        ),
        "omitted additionalProperties permits and must retain other unknown members. Code:\n{code}"
    );
    assert!(
        !struct_header(&code, "Bag").contains("Default"),
        "a model with a synthesized required member must not derive Default. Code:\n{code}"
    );
}

#[test]
fn required_undeclared_member_with_true_ap_is_a_value_and_keeps_extras() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "required": ["payload"],
                    "additionalProperties": true
                }
            }
        }
    });

    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains("pub payload: serde_json::Value"),
        "additionalProperties: true must supply serde_json::Value for an undeclared required member. Code:\n{code}"
    );
    assert!(
        code.contains(
            "pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>"
        ),
        "additionalProperties: true must continue to retain arbitrary extra members. Code:\n{code}"
    );
    assert!(
        !struct_header(&code, "Bag").contains("Default"),
        "a model with a synthesized required member must not derive Default. Code:\n{code}"
    );
}

#[test]
fn required_undeclared_member_uses_typed_ap_value_schema() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "required": ["slug"],
                    "additionalProperties": { "type": "string" }
                }
            }
        }
    });

    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains("pub slug: String"),
        "a required undeclared member must use the additionalProperties value type. Code:\n{code}"
    );
    assert!(
        code.contains("pub additional_properties: std::collections::BTreeMap<String, String>"),
        "other additional members must retain the same declared value type. Code:\n{code}"
    );
    assert!(
        !struct_header(&code, "Bag").contains("Default"),
        "a model with a synthesized required member must not derive Default. Code:\n{code}"
    );
}

#[test]
fn required_undeclared_member_with_false_ap_is_reported_unsatisfiable() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Bag": {
                    "type": "object",
                    "required": ["ghost"],
                    "additionalProperties": false
                }
            }
        }
    });

    let error = analyze_error(spec);
    assert!(
        error.contains("Invalid schema"),
        "unexpected error: {error}"
    );
    assert!(
        error.contains("Bag"),
        "error must identify the schema: {error}"
    );
    assert!(
        error.contains("ghost"),
        "error must identify the impossible required member: {error}"
    );
    assert!(
        error.contains("additionalProperties: false"),
        "error must identify the conflicting constraint: {error}"
    );
    assert!(
        error.to_ascii_lowercase().contains("unsatisfiable"),
        "error must clearly report that the object schema is unsatisfiable: {error}"
    );
}

#[test]
fn allof_required_member_declared_by_sibling_keeps_its_declared_type() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "RequiredOnly": {
                    "type": "object",
                    "required": ["id"]
                },
                "Bag": {
                    "allOf": [
                        {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" }
                            }
                        },
                        { "$ref": "#/components/schemas/RequiredOnly" }
                    ]
                },
                "OnlyRequiredBag": {
                    "allOf": [
                        {
                            "type": "object",
                            "required": ["payload"]
                        }
                    ]
                }
            }
        }
    });

    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    let bag = struct_source(&code, "Bag");
    assert!(
        bag.contains("pub id: String"),
        "required names must be reconciled after allOf sibling properties are merged. Code:\n{code}"
    );
    assert!(
        !bag.contains("pub id: serde_json::Value"),
        "the required-only allOf branch must not prematurely synthesize an untyped field. Code:\n{code}"
    );
    assert!(
        !struct_header(&code, "Bag").contains("Default"),
        "the merged required field must prevent Default. Code:\n{code}"
    );
    let only_required = struct_source(&code, "OnlyRequiredBag");
    assert!(
        only_required.contains("pub payload: serde_json::Value"),
        "an allOf with only an undeclared required member must still materialize an object field. Code:\n{code}"
    );
}

#[test]
fn required_undeclared_members_round_trip_untyped_and_typed_values() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "ap round trip", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            "ValueBag": {
                "type": "object",
                "required": ["payload"]
            },
            "StringBag": {
                "type": "object",
                "required": ["slug"],
                "additionalProperties": { "type": "string" }
            }
        } }
    });
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    let output_dir = temp.path().join("src/generated");
    let mut analyzer = SchemaAnalyzer::new(spec).expect("valid round-trip spec");
    let mut analysis = analyzer.analyze().expect("analyze round-trip spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "required_unknown".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator
        .generate_all(&mut analysis)
        .expect("generate round-trip models");
    generator
        .write_files(&result)
        .expect("write generated round-trip models");

    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "required-unknown-roundtrip-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
use super::generated;

#[test]
fn required_unknown_members_survive_serde_round_trips() {
    let payloads = [
        serde_json::Value::Null,
        serde_json::json!(true),
        serde_json::json!(42),
        serde_json::json!("text"),
        serde_json::json!([1, 2]),
        serde_json::json!({"nested": "value"}),
    ];
    for payload in payloads {
        let input = serde_json::json!({
            "payload": payload,
            "another": {"preserved": true}
        });
        let hydrated: generated::ValueBag =
            serde_json::from_value(input.clone()).expect("hydrate untyped bag");
        assert_eq!(hydrated.payload, input["payload"]);
        assert_eq!(hydrated.additional_properties["another"], input["another"]);
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
    }
    assert!(serde_json::from_value::<generated::ValueBag>(serde_json::json!({})).is_err());

    let typed_input = serde_json::json!({"slug": "known", "another": "also typed"});
    let typed: generated::StringBag =
        serde_json::from_value(typed_input.clone()).expect("hydrate typed bag");
    assert_eq!(typed.slug, "known");
    assert_eq!(typed.additional_properties["another"], "also typed");
    assert_eq!(serde_json::to_value(typed).unwrap(), typed_input);
    assert!(serde_json::from_value::<generated::StringBag>(
        serde_json::json!({"slug": 7})
    ).is_err());
}
}
"#,
    )
    .expect("write scratch source");

    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/required-unknown-roundtrip-smoke"),
        )
        .output()
        .expect("run generated round-trip tests");
    assert!(
        output.status.success(),
        "generated required-member round-trip tests failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
