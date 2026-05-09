//! Q2.6 — vendor extensions `x-enum-varnames` and
//! `x-enum-descriptions` shape the generated string-enum variants.
//! `x-enum-varnames` overrides the default PascalCase heuristic;
//! `x-enum-descriptions` attaches per-variant doc comments.

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
    type_mapping::TypeEnumsConfig,
};
use serde_json::json;

fn enum_spec(values: serde_json::Value, extensions: serde_json::Value) -> serde_json::Value {
    let mut sample = json!({
        "type": "string",
        "enum": values
    });
    let s_obj = sample.as_object_mut().unwrap();
    if let Some(obj) = extensions.as_object() {
        for (k, v) in obj {
            s_obj.insert(k.clone(), v.clone());
        }
    }
    json!({
        "openapi": "3.1.0",
        "info": { "title": "e", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": { "Status": sample }
        }
    })
}

fn generate(spec: serde_json::Value, types_cfg: TypeMappingConfig) -> String {
    let mapper = TypeMapper::new(types_cfg.clone());
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, mapper).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        types: types_cfg,
        ..Default::default()
    };
    CodeGenerator::new(cfg)
        .generate(&mut analysis)
        .expect("generate")
}

#[test]
fn x_enum_varnames_overrides_default_pascalcase() {
    let code = generate(
        enum_spec(
            json!(["active", "inactive", "pending_review"]),
            json!({
                "x-enum-varnames": ["StatusActive", "StatusInactive", "StatusPendingReview"]
            }),
        ),
        TypeMappingConfig::default(),
    );
    // Variant identifiers come from x-enum-varnames.
    assert!(
        code.contains("StatusActive"),
        "Expected StatusActive variant from x-enum-varnames. Code:\n{code}"
    );
    assert!(
        code.contains("StatusPendingReview"),
        "Expected StatusPendingReview variant. Code:\n{code}"
    );
    // Wire format is preserved via #[serde(rename = "<original>")].
    assert!(
        code.contains(r#"#[serde(rename = "active")]"#),
        "Wire format must be preserved via #[serde(rename = ...)]. Code:\n{code}"
    );
    assert!(
        code.contains(r#"#[serde(rename = "pending_review")]"#),
        "Wire format must be preserved via #[serde(rename = ...)]. Code:\n{code}"
    );
}

#[test]
fn x_enum_descriptions_emit_per_variant_doc_comments() {
    let code = generate(
        enum_spec(
            json!(["fast", "slow"]),
            json!({
                "x-enum-descriptions": ["Quick path", "Slow path"]
            }),
        ),
        TypeMappingConfig::default(),
    );
    assert!(
        code.contains("Quick path"),
        "Expected variant doc 'Quick path'. Code:\n{code}"
    );
    assert!(
        code.contains("Slow path"),
        "Expected variant doc 'Slow path'. Code:\n{code}"
    );
}

#[test]
fn extension_length_mismatch_is_silently_dropped() {
    // When x-enum-varnames length doesn't match the enum array, the
    // analysis layer warns and drops the extension entirely. Codegen
    // falls back to the default PascalCase heuristic.
    let code = generate(
        enum_spec(
            json!(["a", "b", "c"]),
            json!({
                "x-enum-varnames": ["VariantA", "VariantB"] // length 2, enum length 3
            }),
        ),
        TypeMappingConfig::default(),
    );
    // Default heuristic should produce A/B/C, not VariantA/VariantB.
    assert!(
        !code.contains("VariantA"),
        "Length-mismatched x-enum-varnames must not be applied. Code:\n{code}"
    );
}

#[test]
fn x_enum_varnames_disabled_falls_back_to_heuristic() {
    let mut cfg = TypeMappingConfig::default();
    cfg.enums = Some(TypeEnumsConfig {
        x_enum_varnames: Some(false),
        x_enum_descriptions: None,
    });
    let code = generate(
        enum_spec(
            json!(["active", "inactive"]),
            json!({
                "x-enum-varnames": ["StatusActive", "StatusInactive"]
            }),
        ),
        cfg,
    );
    assert!(
        !code.contains("StatusActive"),
        "x_enum_varnames = false must not honor the extension. Code:\n{code}"
    );
}

#[test]
fn x_enum_descriptions_disabled_drops_doc_comments() {
    let mut cfg = TypeMappingConfig::default();
    cfg.enums = Some(TypeEnumsConfig {
        x_enum_varnames: None,
        x_enum_descriptions: Some(false),
    });
    let code = generate(
        enum_spec(
            json!(["fast", "slow"]),
            json!({ "x-enum-descriptions": ["Quick path", "Slow path"] }),
        ),
        cfg,
    );
    assert!(
        !code.contains("Quick path"),
        "x_enum_descriptions = false must drop doc comments. Code:\n{code}"
    );
}

#[test]
fn no_extensions_renders_default_heuristic() {
    let code = generate(
        enum_spec(json!(["asc", "desc"]), json!({})),
        TypeMappingConfig::default(),
    );
    // No extensions present → to_rust_enum_variant heuristic.
    assert!(
        code.contains("Asc") && code.contains("Desc"),
        "Default heuristic should produce Asc/Desc. Code:\n{code}"
    );
}
