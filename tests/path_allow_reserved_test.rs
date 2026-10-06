//! `allowReserved: true` on a path parameter selects RFC 6570 reserved
//! expansion, so a value such as `XLON:LLOY` keeps its colon. Without it the
//! default segment encoder still percent-encodes every reserved character.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, analysis::SchemaAnalyzer};
use serde_json::{Value, json};
use std::path::PathBuf;

fn config() -> GeneratorConfig {
    GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    }
}

fn spec(allow_reserved: Option<bool>) -> Value {
    let mut param = json!({
        "name": "identifier",
        "in": "path",
        "required": true,
        "schema": {"type": "string"}
    });
    if let Some(flag) = allow_reserved {
        param["allowReserved"] = json!(flag);
    }
    json!({
        "openapi": "3.1.0",
        "info": {"title": "T", "version": "1.0.0"},
        "paths": {
            "/prices/{identifier}": {
                "get": {
                    "operationId": "getPrice",
                    "parameters": [param],
                    "responses": {"200": {"description": "ok"}}
                }
            }
        }
    })
}

fn generate_methods(spec: Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let analysis = analyzer.analyze().expect("analysis");
    CodeGenerator::new(config())
        .generate_operation_methods(&analysis)
        .to_string()
}

fn generate_client(spec: Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let analysis = analyzer.analyze().expect("analysis");
    CodeGenerator::new(config())
        .generate_http_client(&analysis)
        .expect("client generation")
}

fn encoder_source(client: &str, name: &str) -> String {
    let start = client.find(name).expect("encoder is emitted");
    let body = &client[start..];
    body[..body.find("\n}\n").expect("encoder body ends")].to_string()
}

#[test]
fn allow_reserved_path_parameter_uses_reserved_expansion() {
    let code = generate_methods(spec(Some(true)));
    assert!(
        code.contains("__pct_encode_path_reserved (identifier . as_ref ())"),
        "{code}"
    );
    assert!(!code.contains("__pct_encode_path_segment ("), "{code}");
}

#[test]
fn default_path_parameter_keeps_segment_encoding() {
    for flag in [None, Some(false)] {
        let code = generate_methods(spec(flag));
        assert!(
            code.contains("__pct_encode_path_segment (identifier . as_ref ())"),
            "{code}"
        );
        assert!(!code.contains("__pct_encode_path_reserved ("), "{code}");
    }
}

#[test]
fn reserved_encoder_keeps_segment_safe_reserved_bytes_only() {
    let client = generate_client(spec(Some(true)));
    let reserved = encoder_source(&client, "fn __pct_encode_path_reserved");
    for kept in ["b':'", "b'@'", "b'['", "b';'", "b'='", "b'%'"] {
        assert!(
            reserved.contains(kept),
            "{kept} must pass through: {reserved}"
        );
    }
    for encoded in ["b'/'", "b'?'", "b'#'"] {
        assert!(
            !reserved.contains(encoded),
            "{encoded} must stay percent-encoded: {reserved}"
        );
    }
    let default = encoder_source(&client, "fn __pct_encode_path_segment");
    assert!(!default.contains("b':'"), "{default}");
}

#[test]
fn reserved_encoder_is_only_emitted_when_used() {
    for flag in [None, Some(false)] {
        let client = generate_client(spec(flag));
        assert!(!client.contains("__pct_encode_path_reserved"), "{client}");
    }
}
