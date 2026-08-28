use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn structural_fallback_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "discriminator structural fallback", "version": "1" },
        "paths": {},
        "components": { "schemas": {
            "MappedPrimary": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "primary"],
                "properties": {
                    "kind": { "type": "string", "enum": ["mapped"] },
                    "primary": { "type": "string" }
                }
            },
            "TaglessFallback": {
                "type": "object",
                "additionalProperties": true,
                "required": ["fallback"],
                "properties": { "fallback": { "type": "string" } }
            },
            "MappedUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/MappedPrimary" },
                    { "$ref": "#/components/schemas/TaglessFallback" }
                ],
                "discriminator": {
                    "propertyName": "kind",
                    "mapping": {
                        "mapped": "#/components/schemas/MappedPrimary"
                    }
                }
            },
            "OptionalTagged": {
                "type": "object",
                "additionalProperties": false,
                "required": ["track"],
                "properties": {
                    "kind": { "type": "string", "enum": ["track"] },
                    "track": { "type": "string" }
                }
            },
            "RequiredTagged": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "episode"],
                "properties": {
                    "kind": { "type": "string", "enum": ["episode"] },
                    "episode": { "type": "string" }
                }
            },
            "OptionalTagUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/OptionalTagged" },
                    { "$ref": "#/components/schemas/RequiredTagged" }
                ],
                "discriminator": { "propertyName": "kind" }
            },
            "TaglessOnly": {
                "type": "object",
                "additionalProperties": false,
                "required": ["raw"],
                "properties": { "raw": { "type": "string" } }
            },
            "TaglessUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/TaglessOnly" },
                    { "$ref": "#/components/schemas/RequiredTagged" }
                ],
                "discriminator": { "propertyName": "kind" }
            },
            "TaglessTwin": {
                "type": "object",
                "additionalProperties": false,
                "required": ["raw"],
                "properties": { "raw": { "type": "string" } }
            },
            "AmbiguousTaglessUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/TaglessOnly" },
                    { "$ref": "#/components/schemas/TaglessTwin" }
                ],
                "discriminator": { "propertyName": "kind" }
            },
            "AmbiguousTaglessAnyUnion": {
                "anyOf": [
                    { "$ref": "#/components/schemas/TaglessOnly" },
                    { "$ref": "#/components/schemas/TaglessTwin" }
                ],
                "discriminator": { "propertyName": "kind" }
            },
            "OptionalRed": {
                "type": "object",
                "additionalProperties": false,
                "required": ["red"],
                "properties": {
                    "kind": { "type": "string", "const": "red" },
                    "red": { "type": "string" }
                }
            },
            "OptionalBlue": {
                "type": "object",
                "additionalProperties": false,
                "required": ["blue"],
                "properties": {
                    "kind": { "type": "string", "const": "blue" },
                    "blue": { "type": "string" }
                }
            },
            "OptionalPaint": {
                "anyOf": [
                    { "$ref": "#/components/schemas/OptionalRed" },
                    { "$ref": "#/components/schemas/OptionalBlue" }
                ],
                "discriminator": { "propertyName": "kind" }
            },
            "PaintWrapper": {
                "type": "object",
                "additionalProperties": false,
                "required": ["paint"],
                "properties": {
                    "paint": { "$ref": "#/components/schemas/OptionalPaint" }
                }
            },
            "OtherWrapper": {
                "type": "object",
                "additionalProperties": false,
                "required": ["other"],
                "properties": { "other": { "type": "string" } }
            },
            "CanonicalizingAnyUnion": {
                "anyOf": [
                    { "$ref": "#/components/schemas/PaintWrapper" },
                    { "$ref": "#/components/schemas/OtherWrapper" }
                ]
            }
        } }
    })
}

#[test]
fn analysis_marks_declared_and_required_discriminator_fields() {
    let analysis = SchemaAnalyzer::new(structural_fallback_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");
    let SchemaType::DiscriminatedUnion { variants, .. } =
        &analysis.schemas["OptionalTagUnion"].schema_type
    else {
        panic!("OptionalTagUnion should be discriminated");
    };
    assert!(variants[0].discriminator_field_declared);
    assert!(!variants[0].discriminator_field_required);
    assert!(variants[1].discriminator_field_declared);
    assert!(variants[1].discriminator_field_required);

    let SchemaType::DiscriminatedUnion { variants, .. } =
        &analysis.schemas["TaglessUnion"].schema_type
    else {
        panic!("TaglessUnion should be discriminated");
    };
    assert!(!variants[0].discriminator_field_declared);
    assert!(!variants[0].discriminator_field_required);

    let SchemaType::DiscriminatedUnion { exclusive, .. } =
        &analysis.schemas["AmbiguousTaglessAnyUnion"].schema_type
    else {
        panic!("AmbiguousTaglessAnyUnion should be discriminated");
    };
    assert!(!exclusive);
}

#[test]
fn generated_discriminator_dispatch_uses_unique_structural_fallbacks() {
    let mut analysis = SchemaAnalyzer::new(structural_fallback_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("spec should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod structural_fallback_runtime {
    use super::{
        AmbiguousTaglessAnyUnion, AmbiguousTaglessUnion, CanonicalizingAnyUnion, MappedUnion,
        OptionalTagUnion, TaglessUnion,
    };

    #[test]
    fn mapped_fast_path_and_unique_fallback_both_round_trip() {
        let direct = serde_json::json!({"kind": "mapped", "primary": "p"});
        let hydrated: MappedUnion = serde_json::from_value(direct.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), direct);

        let fallback = serde_json::json!({"kind": "mapped", "fallback": "f"});
        let hydrated: MappedUnion = serde_json::from_value(fallback.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), fallback);
    }

    #[test]
    fn missing_tags_only_try_branches_that_do_not_require_them() {
        let optional = serde_json::json!({"track": "t"});
        let hydrated: OptionalTagUnion = serde_json::from_value(optional).unwrap();
        assert_eq!(
            serde_json::to_value(hydrated).unwrap(),
            serde_json::json!({"kind": "track", "track": "t"})
        );

        let tagless = serde_json::json!({"raw": "wire"});
        let hydrated: TaglessUnion = serde_json::from_value(tagless.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), tagless);

        let ambiguous_any = serde_json::json!({"raw": "matches-both"});
        let hydrated: AmbiguousTaglessAnyUnion =
            serde_json::from_value(ambiguous_any.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated).unwrap(), ambiguous_any);
    }

    #[test]
    fn object_anyof_preserves_input_while_allowing_nested_canonical_tags() {
        let input = serde_json::json!({"paint": {"red": "warm"}});
        let hydrated: CanonicalizingAnyUnion = serde_json::from_value(input).unwrap();
        let canonical = serde_json::to_value(hydrated).unwrap();
        assert_eq!(canonical, serde_json::json!({
            "paint": {"kind": "red", "red": "warm"}
        }));

        let hydrated_again: CanonicalizingAnyUnion =
            serde_json::from_value(canonical.clone()).unwrap();
        assert_eq!(serde_json::to_value(hydrated_again).unwrap(), canonical);
    }

    #[test]
    fn unknown_non_string_ambiguous_and_no_match_inputs_are_errors() {
        assert!(serde_json::from_value::<MappedUnion>(serde_json::json!({
            "kind": "unknown", "fallback": "f"
        })).unwrap_err().to_string().contains("unknown discriminator"));
        assert!(serde_json::from_value::<MappedUnion>(serde_json::json!({
            "kind": 7, "fallback": "f"
        })).unwrap_err().to_string().contains("non-string discriminator"));
        assert!(serde_json::from_value::<AmbiguousTaglessUnion>(serde_json::json!({
            "raw": "matches-both"
        })).unwrap_err().to_string().contains("structurally matched both"));
        assert!(serde_json::from_value::<TaglessUnion>(serde_json::json!({
            "unrelated": true
        })).unwrap_err().to_string().contains("no tagless branch matched"));
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "discriminator-structural-fallback-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    fs::create_dir(temp.path().join("src")).expect("create scratch source");
    fs::write(temp.path().join("src/lib.rs"), generated).expect("write generated source");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/discriminator-structural-fallback-smoke"),
        )
        .output()
        .expect("run generated structural fallback test");
    assert!(
        output.status.success(),
        "generated discriminator fallback failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
