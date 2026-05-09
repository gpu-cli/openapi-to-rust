//! Q2.4 — OpenAPI constraint annotations as `/// Constraint: …`
//! doc comments. **No client-side validation**: the generator never
//! emits `#[validate(...)]` attributes or pulls in the `validator`
//! crate. Constraints belong on the wire contract; the server is
//! the source of truth.

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
    type_mapping::{ConstraintMode, TypeConstraintsConfig},
};
use serde_json::json;

fn spec_with_property(prop: serde_json::Value) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "c", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "required": ["value"],
                    "properties": { "value": prop }
                }
            }
        }
    })
}

fn generate(spec: serde_json::Value, types_cfg: TypeMappingConfig) -> String {
    // Threading the *same* TypeMappingConfig into both the analyzer
    // (via TypeMapper) and the generator's GeneratorConfig.types so
    // analysis-time and codegen-time decisions stay consistent. The
    // production CLI does this in src/bin/openapi-to-rust.rs.
    let mapper = TypeMapper::new(types_cfg.clone());
    let mut analyzer = SchemaAnalyzer::with_type_mapper(spec, mapper).expect("analyzer");
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
fn integer_minimum_maximum_emits_doc_comment_by_default() {
    let code = generate(
        spec_with_property(json!({
            "type": "integer",
            "format": "int32",
            "minimum": 0,
            "maximum": 100
        })),
        TypeMappingConfig::default(),
    );
    assert!(
        code.contains("Constraint: minimum=0, maximum=100"),
        "Expected constraint doc comment. Code:\n{code}"
    );
}

#[test]
fn string_min_max_length_and_pattern_render_in_doc() {
    let code = generate(
        spec_with_property(json!({
            "type": "string",
            "minLength": 3,
            "maxLength": 32,
            "pattern": "^[a-z]+$"
        })),
        TypeMappingConfig::default(),
    );
    assert!(
        code.contains("minLength=3"),
        "Expected minLength in constraint doc. Code:\n{code}"
    );
    assert!(
        code.contains("maxLength=32"),
        "Expected maxLength in constraint doc. Code:\n{code}"
    );
    assert!(
        code.contains("pattern=`^[a-z]+$`"),
        "Expected pattern wrapped in backticks. Code:\n{code}"
    );
}

#[test]
fn array_min_max_items_and_unique_items_render_in_doc() {
    let code = generate(
        spec_with_property(json!({
            "type": "array",
            "items": { "type": "string" },
            "minItems": 1,
            "maxItems": 5,
            "uniqueItems": true
        })),
        TypeMappingConfig::default(),
    );
    assert!(
        code.contains("minItems=1"),
        "Expected minItems in doc. Code:\n{code}"
    );
    assert!(
        code.contains("maxItems=5"),
        "Expected maxItems in doc. Code:\n{code}"
    );
    assert!(
        code.contains("uniqueItems=true"),
        "Expected uniqueItems=true in doc. Code:\n{code}"
    );
}

#[test]
fn no_constraints_emits_no_constraint_doc_line() {
    let code = generate(
        spec_with_property(json!({ "type": "integer" })),
        TypeMappingConfig::default(),
    );
    assert!(
        !code.contains("Constraint:"),
        "Field with no constraints must not get a constraint doc. Code:\n{code}"
    );
}

#[test]
fn mode_off_suppresses_doc_comment() {
    let mut cfg = TypeMappingConfig::default();
    cfg.constraints = Some(TypeConstraintsConfig {
        mode: Some(ConstraintMode::Off),
    });
    let code = generate(
        spec_with_property(json!({
            "type": "integer",
            "minimum": 0
        })),
        cfg,
    );
    assert!(
        !code.contains("Constraint:"),
        "mode = off should suppress constraint doc. Code:\n{code}"
    );
}

#[test]
fn pattern_with_triple_slash_is_escaped() {
    // Pathological but legal: a regex containing `///`. Without
    // escaping, the doc comment would terminate early.
    let code = generate(
        spec_with_property(json!({
            "type": "string",
            "pattern": "abc///def"
        })),
        TypeMappingConfig::default(),
    );
    // The literal `///` substring should NOT appear inside the
    // pattern-doc backticks. We escape with a zero-width-space.
    assert!(
        !code.contains("pattern=`abc///def`"),
        "Triple-slash inside pattern must be escaped. Code:\n{code}"
    );
    // The escaped form should still be present.
    assert!(
        code.contains("pattern=`abc/"),
        "Expected escaped pattern to render. Code:\n{code}"
    );
}

#[test]
fn no_validate_attribute_is_ever_emitted() {
    // Regression guard: the generator must never emit
    // #[validate(...)] regardless of input. Client-side validation
    // is intentionally out of scope.
    let code = generate(
        spec_with_property(json!({
            "type": "integer",
            "minimum": 0,
            "maximum": 100
        })),
        TypeMappingConfig::default(),
    );
    assert!(
        !code.contains("#[validate"),
        "Generator must never emit #[validate(...)] — client-side validation is out of scope. Code:\n{code}"
    );
    assert!(
        !code.contains("validator::"),
        "Generator must not reference the validator crate. Code:\n{code}"
    );
}

#[test]
fn float_constraint_renders_with_decimal() {
    let code = generate(
        spec_with_property(json!({
            "type": "number",
            "format": "double",
            "minimum": 0.5,
            "maximum": 99.95
        })),
        TypeMappingConfig::default(),
    );
    assert!(
        code.contains("minimum=0.5"),
        "Expected float minimum. Code:\n{code}"
    );
    assert!(
        code.contains("maximum=99.95"),
        "Expected float maximum. Code:\n{code}"
    );
}
