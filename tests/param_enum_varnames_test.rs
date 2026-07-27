//! `x-enum-varnames` on parameter-level inline enums.
//!
//! Schema-level enums already honored the extension via
//! `SchemaAnalysis::enum_extensions`, but parameter enums are inline and have
//! no analyzed-schema name to key on, so they silently fell back to the naming
//! heuristic. The same enum therefore produced different Rust variant names
//! depending on whether it lived in `components.schemas` or on a parameter.
//!
//! These drive the client generator directly: parameter enums are emitted into
//! `client.rs`, not `types.rs`.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn generate_client(spec: serde_json::Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer");
    let analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(GeneratorConfig::default());
    generator
        .generate_http_client(&analysis)
        .expect("client generation")
}

fn spec_with_param_enum(extra: serde_json::Value) -> serde_json::Value {
    let mut schema = json!({
        "type": "string",
        "enum": ["gpu-1x-a100", "gpu-8x-h100"]
    });
    if let (Some(target), Some(source)) = (schema.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }

    json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0"},
        "paths": {
            "/instances": {
                "get": {
                    "operationId": "listInstances",
                    "parameters": [{
                        "name": "instanceType",
                        "in": "query",
                        "schema": schema
                    }],
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": {"schema": {"type": "string"}}
                            }
                        }
                    }
                }
            }
        }
    })
}

#[test]
fn parameter_enum_honors_x_enum_varnames() {
    let result = generate_client(spec_with_param_enum(json!({
        "x-enum-varnames": ["SingleA100", "OctoH100"]
    })));

    assert!(
        result.contains("SingleA100"),
        "vendor-supplied variant name must be used, got:\n{result}"
    );
    assert!(
        result.contains("OctoH100"),
        "vendor-supplied variant name must be used, got:\n{result}"
    );
    // The wire value must still be what serde sends.
    assert!(
        result.contains("gpu-1x-a100"),
        "serde rename must still target the wire value, got:\n{result}"
    );
}

/// Without the extension the heuristic still applies — pre-existing behavior
/// that must not change.
#[test]
fn parameter_enum_without_extension_uses_heuristic() {
    let result = generate_client(spec_with_param_enum(json!({})));

    assert!(
        result.contains("Gpu1xA100"),
        "heuristic naming must still apply when no extension is present, got:\n{result}"
    );
}

/// A `x-enum-varnames` array whose length disagrees with `enum` is ambiguous
/// about which name belongs to which value, so it is dropped rather than
/// applied positionally to a prefix.
#[test]
fn mismatched_varnames_length_falls_back_to_heuristic() {
    let result = generate_client(spec_with_param_enum(json!({
        "x-enum-varnames": ["OnlyOneName"]
    })));

    assert!(
        result.contains("Gpu1xA100"),
        "a length-mismatched extension must fall back to the heuristic, got:\n{result}"
    );
    assert!(
        !result.contains("OnlyOneName"),
        "a length-mismatched extension must not be applied, got:\n{result}"
    );
}
