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

use openapi_to_rust::type_mapping::{BinaryStrategy, DateStrategy};
use openapi_to_rust::{
    ByteStrategy, CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
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

fn generate_with_types(spec: serde_json::Value, types: TypeMappingConfig) -> String {
    let mut analyzer =
        SchemaAnalyzer::with_type_mapper(spec, TypeMapper::new(types.clone())).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");
    let codegen = CodeGenerator::new(GeneratorConfig {
        module_name: "sample".into(),
        enable_async_client: false,
        types,
        ..Default::default()
    });
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

fn binary_model_round_trip_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "binary model", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "BinaryBlob": {
                    "type": "string",
                    "format": "binary"
                },
                "BinaryAlias": {
                    "$ref": "#/components/schemas/BinaryBlob"
                },
                "google.protobuf.Any": {
                    "additionalProperties": true,
                    "properties": {
                        "debug": {
                            "additionalProperties": true,
                            "type": "object"
                        },
                        "type": { "type": "string" },
                        "value": { "type": "string", "format": "binary" }
                    },
                    "type": "object"
                },
                "Sample": {
                    "type": "object",
                    "required": ["direct", "aliased", "encoded", "gitpod_any"],
                    "properties": {
                        "direct": { "type": "string", "format": "binary" },
                        "optional": { "type": ["string", "null"], "format": "binary" },
                        "aliased": { "$ref": "#/components/schemas/BinaryAlias" },
                        "encoded": { "type": "string", "format": "byte" },
                        "optional_encoded": { "type": ["string", "null"], "format": "byte" },
                        "gitpod_any": { "$ref": "#/components/schemas/google.protobuf.Any" }
                    }
                }
            }
        }
    })
}

fn assert_generated_binary_model_round_trip(strategy: BinaryStrategy, name: &str) {
    let types = TypeMappingConfig {
        binary: strategy,
        ..TypeMappingConfig::default()
    };
    let code = generate_with_types(binary_model_round_trip_spec(), types);
    let temp = tempfile::TempDir::new().expect("scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(temp.path().join("src/generated.rs"), &code).expect("generated module");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "binary-model-{name}"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
base64 = "0.22"
bytes = {{ version = "1", features = ["serde"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
        ),
    )
    .expect("scratch manifest");
    std::fs::write(
        temp.path().join("src/main.rs"),
        r##"#![allow(dead_code)]
mod generated;

fn main() {
    let input = serde_json::json!({
        "direct": "direct bytes",
        "optional": "optional bytes",
        "aliased": "referenced bytes",
        "encoded": "aGk=",
        "optional_encoded": "aGk=",
        "gitpod_any": {
            "debug": {},
            "type": "type.googleapis.com/example.Message",
            "value": "protobuf bytes",
            "synthetic_extension": true
        }
    });
    let value: generated::Sample = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), input);

    let missing_optional = serde_json::json!({
        "direct": "direct bytes",
        "aliased": "referenced bytes",
        "encoded": "aGk=",
        "gitpod_any": {
            "type": "type.googleapis.com/example.Empty"
        }
    });
    let value: generated::Sample = serde_json::from_value(missing_optional.clone()).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), missing_optional);

    let explicit_nulls = serde_json::json!({
        "direct": "direct bytes",
        "optional": null,
        "aliased": "referenced bytes",
        "encoded": "aGk=",
        "optional_encoded": null,
        "gitpod_any": {
            "type": "type.googleapis.com/example.Empty"
        }
    });
    let value: generated::Sample = serde_json::from_value(explicit_nulls.clone()).unwrap();
    assert_eq!(value.optional, Some(None));
    assert_eq!(value.optional_encoded, Some(None));
    assert_eq!(serde_json::to_value(value).unwrap(), explicit_nulls);
}
"##,
    )
    .expect("scratch main");

    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("target/generated-binary-model-{name}")),
        )
        .status()
        .expect("cargo run");
    assert!(
        status.success(),
        "generated {name} binary model did not round-trip. Code:\n{code}"
    );

    assert!(
        code.contains(r#"with = "base64_serde""#),
        "format: byte must remain base64-encoded under {name}. Code:\n{code}"
    );
}

#[test]
fn generated_binary_model_fields_round_trip_as_json_strings_for_every_strategy() {
    for (name, strategy, codec) in [
        ("bytes", BinaryStrategy::Bytes, Some("binary_bytes_serde")),
        ("vec-u8", BinaryStrategy::VecU8, Some("binary_vec_serde")),
        ("string", BinaryStrategy::String, None),
    ] {
        let types = TypeMappingConfig {
            binary: strategy,
            ..TypeMappingConfig::default()
        };
        let code = generate_with_types(binary_model_round_trip_spec(), types);
        match codec {
            Some(codec) => {
                assert!(code.contains(&format!("mod {codec}")), "Code:\n{code}");
                assert!(
                    code.matches(&format!(r#"with = "{codec}""#)).count() >= 2,
                    "direct and referenced required fields need the codec. Code:\n{code}"
                );
                assert!(
                    code.contains(&format!(r#"with = "{codec}::option""#)),
                    "optional field needs the option codec. Code:\n{code}"
                );
            }
            None => {
                assert!(!code.contains("mod binary_bytes_serde"), "Code:\n{code}");
                assert!(!code.contains("mod binary_vec_serde"), "Code:\n{code}");
            }
        }
        assert_generated_binary_model_round_trip(strategy, name);
    }
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
    assert!(code.contains("STANDARD as ENGINE"), "Code:\n{code}");
    assert!(!code.contains("URL_SAFE_NO_PAD"), "Code:\n{code}");
}

#[test]
fn byte_url_unpadded_emits_rfc7515_codec() {
    let code = generate_with_types(
        spec_with_format("byte"),
        TypeMappingConfig {
            byte: ByteStrategy::Base64UrlUnpadded,
            ..TypeMappingConfig::default()
        },
    );
    assert!(code.contains("URL_SAFE_NO_PAD as ENGINE"), "Code:\n{code}");
    assert!(!code.contains("STANDARD as ENGINE"), "Code:\n{code}");
}

#[test]
fn generated_byte_url_unpadded_codec_round_trips() {
    let code = generate_with_types(
        spec_with_format("byte"),
        TypeMappingConfig {
            byte: ByteStrategy::Base64UrlUnpadded,
            ..TypeMappingConfig::default()
        },
    );
    let temp = tempfile::TempDir::new().expect("scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(temp.path().join("src/generated.rs"), code).expect("generated module");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "byte-url-unpadded-roundtrip"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
base64 = "0.22"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("scratch manifest");
    std::fs::write(
        temp.path().join("src/main.rs"),
        r##"#![allow(dead_code)]
mod generated;

fn main() {
    let value = generated::Sample { value: vec![251, 255] };
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, r#"{"value":"-_8"}"#);
    let decoded: generated::Sample = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.value, vec![251, 255]);
}
"##,
    )
    .expect("scratch main");

    let status = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/generated-byte-roundtrip"),
        )
        .status()
        .expect("cargo run");
    assert!(
        status.success(),
        "generated URL-safe codec did not round-trip"
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
// GH #25: DateStrategy::Time for `format: date`. `time::serde::iso8601`
// only supports OffsetDateTime, so the generator must emit its own date codec.
// JSON Schema `format: time` is RFC 3339 full-time and stays a string because
// both chrono::NaiveTime and time::Time lack its required UTC offset.
// =====================================================================

fn time_strategy_mapper() -> TypeMapper {
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

fn spec_with_optional_nullable_format(format: &str) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "fmt", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "properties": {
                        "value": { "type": ["string", "null"], "format": format }
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
fn time_with_time_strategy_preserves_rfc3339_offset_as_string() {
    let code = generate(spec_with_optional_format("time"), time_strategy_mapper());
    assert!(
        code.contains("pub value: Option<String>"),
        "RFC 3339 full-time must retain its offset-bearing string. Code:\n{code}"
    );
    assert!(
        !code.contains("time_time_format") && !code.contains("NaiveTime"),
        "an offset-less time codec must not be emitted. Code:\n{code}"
    );
}

#[test]
fn nullable_time_scalars_compose_their_codecs_with_field_presence() {
    for (format, rust_type, codec) in [
        ("date", "time::Date", "time_date_double_option"),
        (
            "date-time",
            "time::OffsetDateTime",
            "time_rfc3339_double_option",
        ),
    ] {
        let code = generate(
            spec_with_optional_nullable_format(format),
            time_strategy_mapper(),
        );
        assert!(
            code.contains(&format!("pub value: Option<Option<{rust_type}>>")),
            "optional nullable {format} must retain both presence and nullability. Code:\n{code}"
        );
        assert!(
            code.contains(&format!(r#"with = "{codec}""#)),
            "optional nullable {format} must use its double-option codec. Code:\n{code}"
        );
    }

    let time = generate(
        spec_with_optional_nullable_format("time"),
        time_strategy_mapper(),
    );
    assert!(time.contains("pub value: Option<Option<String>>"));
    assert!(time.contains(r#"deserialize_with = "tri_state_serde::deserialize""#));
    assert!(!time.contains("time_time_format"));
}

fn rfc3339_full_time_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "RFC 3339 full-time", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            "Clock": {
                "type": "object",
                "additionalProperties": false,
                "required": ["required", "series", "by_name"],
                "properties": {
                    "required": { "type": "string", "format": "time" },
                    "optional": { "type": ["string", "null"], "format": "time" },
                    "series": {
                        "type": "array",
                        "items": { "type": "string", "format": "time" }
                    },
                    "by_name": {
                        "type": "object",
                        "additionalProperties": { "type": "string", "format": "time" }
                    }
                }
            }
        } }
    })
}

fn assert_generated_rfc3339_time_round_trip(strategy: DateStrategy, name: &str) {
    let code = generate_with_types(
        rfc3339_full_time_spec(),
        TypeMappingConfig {
            time: strategy,
            ..TypeMappingConfig::default()
        },
    );
    let compact = code.split_whitespace().collect::<String>();
    assert!(compact.contains("pubrequired:String"));
    assert!(compact.contains("puboptional:Option<Option<String>>"));
    assert!(compact.contains("pubseries:Vec<String>"));
    assert!(compact.contains("BTreeMap<String,String>"));
    assert!(!code.contains("NaiveTime") && !code.contains("time::Time"));

    let temp = tempfile::TempDir::new().expect("RFC 3339 time scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/generated.rs"), code).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "rfc3339-time-{name}"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/main.rs"),
        r#"mod generated;

fn round_trip(input: serde_json::Value) {
    let hydrated: generated::Clock = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
}

fn main() {
    round_trip(serde_json::json!({
        "required": "03:04:05Z",
        "optional": "03:04:05.123+05:30",
        "series": ["23:59:59-07:00", "00:00:00.000001+14:00"],
        "by_name": {"negative": "12:34:56.789-03:30"}
    }));
    round_trip(serde_json::json!({
        "required": "03:04:05+00:00",
        "series": [],
        "by_name": {}
    }));
    round_trip(serde_json::json!({
        "required": "03:04:05Z",
        "optional": null,
        "series": ["03:04:05Z"],
        "by_name": {}
    }));
}
"#,
    )
    .unwrap();

    let output = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("target/generated-rfc3339-time-{name}")),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated RFC 3339 time model failed for {name}:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn every_time_strategy_round_trips_rfc3339_offsets_exactly() {
    for (strategy, name) in [
        (DateStrategy::String, "string"),
        (DateStrategy::Chrono, "chrono"),
        (DateStrategy::Time, "time"),
    ] {
        assert_generated_rfc3339_time_round_trip(strategy, name);
    }
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
