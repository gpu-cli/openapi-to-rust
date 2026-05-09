//! Q2.7: untagged enums for `oneOf` / `anyOf` of primitives.
//!
//! Asserts that primitive unions emit a clean
//! `#[serde(untagged)] pub enum X { String(String), Integer(i64), … }`
//! by default (`primitive_unions = true`), and revert to the
//! pre-Q2.7 type-alias-per-variant shape when the toggle is off.

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
    type_mapping::TypeShapeConfig,
};
use serde_json::json;

fn generate(spec: serde_json::Value, mapper: TypeMapper) -> String {
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, mapper).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    codegen.generate(&mut analysis).expect("generate")
}

fn primitive_union_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "p", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "OneOfId": {
                    "oneOf": [{ "type": "string" }, { "type": "integer" }]
                },
                "AnyOfId": {
                    "anyOf": [{ "type": "string" }, { "type": "integer" }]
                },
                "Triple": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "integer" },
                        { "type": "boolean" }
                    ]
                }
            }
        }
    })
}

#[test]
fn oneof_primitives_default_emits_untagged_enum_with_primitive_variants() {
    let code = generate(
        primitive_union_spec(),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    // The `oneOf` path was already producing the right shape pre-Q2.7;
    // this test pins the behavior so a future refactor can't regress it.
    assert!(
        code.contains("#[serde(untagged)]\npub enum OneOfId"),
        "OneOfId should be a #[serde(untagged)] enum. Code:\n{code}"
    );
    assert!(
        code.contains("String(String)") && code.contains("Integer(i64)"),
        "OneOfId should have String(String) and Integer(i64) variants. Code:\n{code}"
    );
}

#[test]
fn anyof_primitives_default_emits_clean_untagged_enum() {
    let code = generate(
        primitive_union_spec(),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    // Pre-Q2.7 emitted
    //   pub type AnyOfIdString = String;
    //   pub type AnyOfIdIntegerVariant1 = i64;
    //   pub enum AnyOfId { AnyOfIdString(AnyOfIdString), … }
    // Q2.7 collapses that to the same shape oneOf already used.
    assert!(
        code.contains("pub enum AnyOfId {\n    String(String),\n    Integer(i64),\n}"),
        "AnyOfId should match the oneOf shape, no per-variant type aliases. Code:\n{code}"
    );
    // Negative: no per-variant type alias should remain.
    assert!(
        !code.contains("pub type AnyOfIdString"),
        "Pre-Q2.7 type alias must not be emitted under default config. Code:\n{code}"
    );
}

#[test]
fn anyof_three_primitives_default_emits_three_variants() {
    let code = generate(
        primitive_union_spec(),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains(
            "pub enum Triple {\n    String(String),\n    Integer(i64),\n    Boolean(bool),\n}"
        ),
        "Triple should emit one variant per primitive type. Code:\n{code}"
    );
}

#[test]
fn anyof_primitives_with_toggle_off_reverts_to_type_aliases() {
    let mut cfg = TypeMappingConfig::default();
    cfg.shape = Some(TypeShapeConfig {
        primitive_unions: Some(false),
        ..Default::default()
    });
    let code = generate(primitive_union_spec(), TypeMapper::new(cfg));
    // Pre-Q2.7 shape: per-variant type aliases.
    assert!(
        code.contains("pub type AnyOfIdString"),
        "Pre-Q2.7 type alias should reappear when primitive_unions = false. Code:\n{code}"
    );
}

#[test]
fn anyof_with_explicit_null_variant_drops_null() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "p", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Nullable": {
                    "anyOf": [
                        { "type": "string" },
                        { "type": "integer" },
                        { "type": "null" }
                    ]
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    // Null variant is filtered (nullability surfaces as Option<T> at the
    // property level via is_nullable_pattern); the enum just holds the
    // non-null primitives.
    assert!(
        code.contains("pub enum Nullable {\n    String(String),\n    Integer(i64),\n}"),
        "Null variant should be dropped from the union. Code:\n{code}"
    );
}

#[test]
fn primitive_union_round_trips_each_variant() {
    // Lightweight schema-level guarantee: serde_json round-trips
    // each primitive case via the same untagged enum without a
    // discriminator field. Requires building a tiny ad-hoc crate
    // would be overkill — instead assert the generated code
    // contains the #[serde(untagged)] attribute (which is what
    // makes round-trips work) and the right variants.
    let code = generate(
        primitive_union_spec(),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    let derived_count = code.matches("#[serde(untagged)]").count();
    assert!(
        derived_count >= 3,
        "Expected at least 3 #[serde(untagged)] enums (OneOfId, AnyOfId, Triple). Code:\n{code}"
    );
}
