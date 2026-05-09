//! End-to-end checks for Q2 typed-scalar generation.
//!
//! Asserts that an OpenAPI property declared as `type: string,
//! format: <X>` lands in the generated Rust as the right typed
//! scalar (chrono::DateTime, uuid::Uuid, …) under the default
//! [`TypeMappingConfig`] and as plain `String` under
//! `TypeMappingConfig::conservative()`.
//!
//! Lives at the integration layer because the wiring crosses
//! `analysis.rs`, `generator.rs`, and `type_mapping.rs`; a unit test
//! only on `TypeMapper` would miss the codec threading through
//! `SchemaType::Primitive.serde_with`.

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
};
use serde_json::json;

fn spec_with_format(format: &str) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "fmt", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "required": ["value"],
                    "properties": {
                        "value": { "type": "string", "format": format }
                    }
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
    let codegen = CodeGenerator::new(cfg);
    codegen.generate(&mut analysis).expect("generate")
}

#[test]
fn date_time_default_emits_chrono_datetime() {
    let code = generate(
        spec_with_format("date-time"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: chrono::DateTime<chrono::Utc>"),
        "date-time should map to chrono::DateTime<Utc> by default. Code:\n{code}"
    );
}

#[test]
fn date_time_conservative_emits_string() {
    let code = generate(
        spec_with_format("date-time"),
        TypeMapper::new(TypeMappingConfig::conservative()),
    );
    assert!(
        code.contains("pub value: String"),
        "date-time with conservative config should be String. Code:\n{code}"
    );
    assert!(
        !code.contains("chrono::"),
        "conservative config must not reference chrono. Code:\n{code}"
    );
}

#[test]
fn uuid_default_emits_uuid_uuid() {
    let code = generate(
        spec_with_format("uuid"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: uuid::Uuid"),
        "uuid should map to uuid::Uuid by default. Code:\n{code}"
    );
}

#[test]
fn uri_default_emits_url_url() {
    let code = generate(
        spec_with_format("uri"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: url::Url"),
        "uri should map to url::Url by default. Code:\n{code}"
    );
}

#[test]
fn ipv4_default_emits_std_net_ipv4addr() {
    let code = generate(
        spec_with_format("ipv4"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: std::net::Ipv4Addr"),
        "ipv4 should map to std::net::Ipv4Addr by default. Code:\n{code}"
    );
}

#[test]
fn binary_default_emits_bytes_bytes() {
    let code = generate(
        spec_with_format("binary"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: bytes::Bytes"),
        "binary should map to bytes::Bytes by default. Code:\n{code}"
    );
}

#[test]
fn byte_default_emits_vec_u8_with_base64_codec() {
    let code = generate(
        spec_with_format("byte"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    // Type
    assert!(
        code.contains("pub value: Vec<u8>"),
        "byte should map to Vec<u8>. Code:\n{code}"
    );
    // Codec attribute on the field
    assert!(
        code.contains(r#"with = "base64_serde""#),
        "byte field should carry #[serde(with = \"base64_serde\")]. Code:\n{code}"
    );
    // Helper module emitted exactly once
    assert!(
        code.contains("mod base64_serde"),
        "Generated file should include the base64_serde helper module. Code:\n{code}"
    );
}

#[test]
fn no_byte_format_no_base64_helper_emitted() {
    // Sanity: helper module is gated on actual usage, so a spec
    // that uses date-time/uuid but never byte must not include it.
    let code = generate(
        spec_with_format("date-time"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        !code.contains("mod base64_serde"),
        "base64_serde must not be emitted when no field uses format: byte. Code:\n{code}"
    );
}

#[test]
fn unknown_format_falls_through_to_string() {
    let code = generate(
        spec_with_format("hostname"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: String"),
        "Unknown format should fall through to String. Code:\n{code}"
    );
}

#[test]
fn required_deps_are_populated_for_typed_scalars() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "fmt", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "required": ["a", "b", "c", "d"],
                    "properties": {
                        "a": { "type": "string", "format": "date-time" },
                        "b": { "type": "string", "format": "uuid" },
                        "c": { "type": "string", "format": "uri" },
                        "d": { "type": "string", "format": "byte" }
                    }
                }
            }
        }
    });
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::default()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");

    let crate_names: Vec<&str> = result.required_deps.iter().map(|d| d.crate_name).collect();
    // Sorted, deterministic ordering.
    assert_eq!(crate_names, vec!["base64", "chrono", "url", "uuid"]);
}

#[test]
fn required_deps_empty_for_pure_string_spec() {
    let spec = spec_with_format("hostname"); // unknown format → String
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::default()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");

    assert!(
        result.required_deps.is_empty(),
        "spec with no typed scalars should have empty required_deps. Got: {:?}",
        result.required_deps
    );
}

#[test]
fn write_files_drops_required_deps_toml_when_typed_scalars_used() {
    let spec = spec_with_format("date-time");
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::default()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");

    let temp = tempfile::TempDir::new().expect("temp");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        output_dir: temp.path().into(),
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");
    codegen.write_files(&result).expect("write_files");

    let deps_path = temp.path().join("REQUIRED_DEPS.toml");
    assert!(
        deps_path.exists(),
        "REQUIRED_DEPS.toml should be written when typed scalars are used"
    );
    let body = std::fs::read_to_string(&deps_path).expect("read deps file");
    assert!(body.contains("[dependencies]"), "body:\n{body}");
    assert!(body.contains("chrono = "), "body:\n{body}");
    assert!(
        body.contains("# Generated by openapi-to-rust"),
        "should include explanatory header. body:\n{body}"
    );
}

#[test]
fn write_files_skips_required_deps_toml_when_no_typed_scalars() {
    let spec = spec_with_format("hostname");
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::default()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");

    let temp = tempfile::TempDir::new().expect("temp");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        output_dir: temp.path().into(),
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");
    codegen.write_files(&result).expect("write_files");

    let deps_path = temp.path().join("REQUIRED_DEPS.toml");
    assert!(
        !deps_path.exists(),
        "REQUIRED_DEPS.toml should NOT be written when no typed scalars are used"
    );
}

#[test]
fn no_format_property_remains_string() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "fmt", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "required": ["value"],
                    "properties": {
                        "value": { "type": "string" }
                    }
                }
            }
        }
    });
    let code = generate(spec, TypeMapper::new(TypeMappingConfig::default()));
    assert!(
        code.contains("pub value: String"),
        "string with no format must remain String. Code:\n{code}"
    );
}
