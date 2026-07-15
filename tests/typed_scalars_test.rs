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
        enable_async_client: false,
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
        enable_async_client: false,
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");

    let crate_names: Vec<&str> = result.required_deps.iter().map(|d| d.crate_name).collect();
    // Sorted, deterministic ordering.
    assert_eq!(
        crate_names,
        vec!["base64", "chrono", "serde", "url", "uuid"]
    );
}

#[test]
fn required_deps_include_serde_for_pure_string_spec() {
    let spec = spec_with_format("hostname"); // unknown format → String
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::default()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        enable_async_client: false,
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");

    assert_eq!(
        result
            .required_deps
            .iter()
            .map(|dependency| dependency.crate_name)
            .collect::<Vec<_>>(),
        vec!["serde"]
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
        enable_async_client: false,
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
fn write_files_emits_required_deps_toml_for_base_serde_dependency() {
    let spec = spec_with_format("hostname");
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::default()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");

    let temp = tempfile::TempDir::new().expect("temp");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        output_dir: temp.path().into(),
        enable_async_client: false,
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");
    codegen.write_files(&result).expect("write_files");

    let deps_path = temp.path().join("REQUIRED_DEPS.toml");
    let body = std::fs::read_to_string(deps_path).expect("complete dependency fragment");
    assert!(body.contains("serde = { version = \"1\", features = [\"derive\"] }"));
    assert!(!body.contains("reqwest ="), "types-only output: {body}");
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

// =====================================================================
// GH #25: DateStrategy::Time for `format: date` / `format: time`.
// `time::serde::iso8601` only supports OffsetDateTime, so the
// generator must emit its own codec modules via
// `time::serde::format_description!` instead.
// =====================================================================

fn time_strategy_mapper() -> TypeMapper {
    use openapi_to_rust::type_mapping::DateStrategy;
    TypeMapper::new(TypeMappingConfig {
        date_time: DateStrategy::Time,
        date: DateStrategy::Time,
        time: DateStrategy::Time,
        ..TypeMappingConfig::default()
    })
}

/// The exact shape from GH #25: an *optional* `format: date` field.
fn spec_with_optional_format(format: &str) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "fmt", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string", "format": format }
                    }
                }
            }
        }
    })
}

#[test]
fn date_with_time_strategy_emits_generated_codec() {
    let code = generate(spec_with_format("date"), time_strategy_mapper());
    assert!(
        code.contains("pub value: time::Date"),
        "date should map to time::Date under DateStrategy::Time. Code:\n{code}"
    );
    assert!(
        code.contains(r#"with = "time_date_format""#),
        "required date field should use the generated codec. Code:\n{code}"
    );
    assert!(
        code.contains("time::serde::format_description!"),
        "the codec module declaration must be emitted. Code:\n{code}"
    );
    assert!(
        !code.contains("time::serde::iso8601"),
        "iso8601 codec is OffsetDateTime-only and must not appear (GH #25). Code:\n{code}"
    );
}

#[test]
fn optional_date_with_time_strategy_uses_option_codec() {
    let code = generate(spec_with_optional_format("date"), time_strategy_mapper());
    assert!(
        code.contains("pub value: Option<time::Date>"),
        "optional date should be Option<time::Date>. Code:\n{code}"
    );
    assert!(
        code.contains(r#"with = "time_date_format::option""#),
        "optional date field should use the ::option submodule. Code:\n{code}"
    );
}

#[test]
fn time_with_time_strategy_emits_generated_codec() {
    let code = generate(spec_with_optional_format("time"), time_strategy_mapper());
    assert!(
        code.contains("pub value: Option<time::Time>"),
        "optional time should be Option<time::Time>. Code:\n{code}"
    );
    assert!(
        code.contains(r#"with = "time_time_format::option""#),
        "optional time field should use the ::option submodule. Code:\n{code}"
    );
    assert!(
        code.contains("time_time_format"),
        "the time codec module declaration must be emitted. Code:\n{code}"
    );
}

#[test]
fn date_time_with_time_strategy_does_not_emit_codec_modules() {
    // OffsetDateTime has a built-in rfc3339 codec; the generated
    // format_description modules are gated on date/time usage.
    let code = generate(spec_with_format("date-time"), time_strategy_mapper());
    assert!(
        code.contains(r#"with = "time::serde::rfc3339""#),
        "date-time keeps the built-in rfc3339 codec. Code:\n{code}"
    );
    assert!(
        !code.contains("format_description!"),
        "no codec module should be emitted without date/time fields. Code:\n{code}"
    );
}

#[test]
fn time_dep_requirement_includes_macro_features() {
    // The generated format_description! invocation needs `macros`;
    // even rfc3339 needs formatting+parsing. A bare `serde` feature
    // produces code that doesn't compile.
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "fmt", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "required": ["a"],
                    "properties": {
                        "a": { "type": "string", "format": "date-time" },
                        "b": { "type": "string", "format": "date" }
                    }
                }
            }
        }
    });
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, time_strategy_mapper()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let cfg = GeneratorConfig {
        module_name: "sample".into(),
        ..Default::default()
    };
    let codegen = CodeGenerator::new(cfg);
    let result = codegen.generate_all(&mut analysis).expect("generate_all");

    // One merged `time` line carrying the union of required features.
    let time_deps: Vec<_> = result
        .required_deps
        .iter()
        .filter(|d| d.crate_name == "time")
        .collect();
    assert_eq!(time_deps.len(), 1, "deps: {:?}", result.required_deps);
    for feat in ["serde", "formatting", "parsing", "macros"] {
        assert!(
            time_deps[0].features.contains(&feat),
            "time dep must include feature {feat}. Got: {:?}",
            time_deps[0].features
        );
    }
}
