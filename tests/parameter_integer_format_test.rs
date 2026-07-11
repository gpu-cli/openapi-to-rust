//! Regression test for https://github.com/gpu-cli/openapi-to-rust/issues/23
//!
//! Query/path parameters previously ignored the schema `format`: `analyze_parameter`
//! hardcoded `integer => i64` and `number => f64`, bypassing the `TypeMapper` that
//! the schema-property path already uses. As a result a `format: int32` query
//! parameter always rendered as `i64` and no `[type_mappings]`/strategy config could
//! change it. Parameters must resolve their scalar type through the same mapper as
//! properties.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, analysis::SchemaAnalyzer};
use serde_json::json;
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

fn generate(spec: serde_json::Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(config());
    generator.generate_operation_methods(&analysis).to_string()
}

fn spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": {"title": "T", "version": "1.0.0"},
        "components": {"schemas": {"Stub": {"type": "object", "properties": {"id": {"type": "string"}}}}},
        "paths": {
            "/items": {
                "get": {
                    "operationId": "listItems",
                    "parameters": [
                        {
                            "name": "limit",
                            "in": "query",
                            "required": true,
                            "schema": {
                                "type": "integer",
                                "format": "int32",
                                "default": 20,
                                "maximum": 100,
                                "minimum": 1
                            }
                        },
                        {
                            "name": "offset",
                            "in": "query",
                            "required": true,
                            "schema": {"type": "integer", "format": "int64"}
                        },
                        {
                            "name": "cursor",
                            "in": "query",
                            "required": true,
                            "schema": {"type": "integer"}
                        },
                        {
                            "name": "ratio",
                            "in": "query",
                            "required": true,
                            "schema": {"type": "number", "format": "float"}
                        },
                        {
                            "name": "weight",
                            "in": "query",
                            "required": true,
                            "schema": {"type": "number", "format": "double"}
                        }
                    ],
                    "responses": {"200": {"description": "ok"}}
                }
            }
        }
    })
}

#[test]
fn int32_query_parameter_resolves_to_i32() {
    let code = generate(spec());
    assert!(
        code.contains("limit : i32"),
        "int32 query parameter should render as i32; got:\n{code}"
    );
    assert!(
        !code.contains("limit : i64"),
        "int32 query parameter must NOT fall back to i64; got:\n{code}"
    );
}

#[test]
fn int64_and_formatless_integer_parameters_stay_i64() {
    let code = generate(spec());
    assert!(
        code.contains("offset : i64"),
        "int64 query parameter should render as i64; got:\n{code}"
    );
    // No format defaults to i64, matching the schema-property path.
    assert!(
        code.contains("cursor : i64"),
        "formatless integer query parameter should default to i64; got:\n{code}"
    );
}

#[test]
fn number_format_parameters_resolve_through_type_mapper() {
    let code = generate(spec());
    assert!(
        code.contains("ratio : f32"),
        "float query parameter should render as f32; got:\n{code}"
    );
    assert!(
        code.contains("weight : f64"),
        "double query parameter should render as f64; got:\n{code}"
    );
}
