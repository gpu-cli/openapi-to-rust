//! Q2.1 + Q2.2: end-to-end checks that unsigned integer formats
//! and built-in format aliases reach the generated Rust.

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, TypeMapper, TypeMappingConfig,
};
use serde_json::json;

fn integer_spec(format: &str) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "ints", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Sample": {
                    "type": "object",
                    "required": ["value"],
                    "properties": {
                        "value": { "type": "integer", "format": format }
                    }
                }
            }
        }
    })
}

fn string_spec(format: &str) -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "strs", "version": "1.0.0" },
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
    CodeGenerator::new(cfg)
        .generate(&mut analysis)
        .expect("generate")
}

#[test]
fn uint32_default_emits_u32() {
    let code = generate(
        integer_spec("uint32"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: u32"),
        "uint32 should map to u32 by default. Code:\n{code}"
    );
}

#[test]
fn uint64_default_emits_u64() {
    let code = generate(
        integer_spec("uint64"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: u64"),
        "uint64 should map to u64 by default. Code:\n{code}"
    );
}

#[test]
fn unsigned_off_degrades_uint64_to_i64() {
    let mut cfg = TypeMappingConfig::default();
    cfg.unsigned = false;
    let code = generate(integer_spec("uint64"), TypeMapper::new(cfg));
    assert!(
        code.contains("pub value: i64"),
        "unsigned = false should fall back to i64 for uint64. Code:\n{code}"
    );
}

#[test]
fn int32_int64_unchanged() {
    let code = generate(
        integer_spec("int32"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: i32"),
        "int32 should still map to i32. Code:\n{code}"
    );

    let code = generate(
        integer_spec("int64"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: i64"),
        "int64 should still map to i64. Code:\n{code}"
    );
}

#[test]
fn builtin_alias_uuid4_resolves_to_uuid_uuid() {
    let code = generate(
        string_spec("uuid4"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: uuid::Uuid"),
        "format: uuid4 should normalize to uuid::Uuid via built-in alias. Code:\n{code}"
    );
}

#[test]
fn builtin_alias_unix_time_resolves_to_i64() {
    let code = generate(
        integer_spec("unix-time"),
        TypeMapper::new(TypeMappingConfig::default()),
    );
    assert!(
        code.contains("pub value: i64"),
        "format: unix-time on integer should normalize to int64 via alias. Code:\n{code}"
    );
}

#[test]
fn user_format_alias_overrides_builtin() {
    let mut cfg = TypeMappingConfig::default();
    cfg.format_aliases
        .insert("uuid4".to_string(), "hostname".to_string());
    let code = generate(string_spec("uuid4"), TypeMapper::new(cfg));
    // hostname is unmapped → falls through to plain String.
    assert!(
        code.contains("pub value: String"),
        "user alias should override built-in. Code:\n{code}"
    );
}

#[test]
fn conservative_disables_uint_and_aliases() {
    // Conservative collapses everything; uint64 falls to i64 and
    // alias paths still normalize but the underlying strategies
    // produce String for typed targets.
    let cfg = TypeMappingConfig::conservative();
    let code = generate(integer_spec("uint64"), TypeMapper::new(cfg.clone()));
    assert!(
        code.contains("pub value: i64"),
        "conservative should keep uint64 as i64. Code:\n{code}"
    );

    let code = generate(string_spec("uuid4"), TypeMapper::new(cfg));
    // Alias still normalizes uuid4→uuid, but uuid strategy is
    // String under conservative, so the final type is String.
    assert!(
        code.contains("pub value: String"),
        "conservative + uuid4 should still render as String. Code:\n{code}"
    );
}
