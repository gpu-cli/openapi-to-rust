//! Regression test for the issue #10 follow-up:
//! https://github.com/gpu-cli/openapi-to-rust/issues/10#issuecomment-4402973418
//!
//! When a path/query parameter's inline schema is `{"type":"string","const":"X"}`
//! the generator should emit a single-variant enum and use it in the function
//! signature, not `impl AsRef<str>`. Same path also fixes parameters that use
//! `enum: ["A","B"]` directly on their inline schema.

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

#[test]
fn const_only_path_parameter_generates_single_variant_enum() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "T", "version": "1.0.0"},
        "components": {"schemas": {"Stub": {"type": "object", "properties": {"id": {"type": "string"}}}}},
        "paths": {
            "/some/const/path/{the_constant}": {
                "get": {
                    "operationId": "someConstOperation",
                    "parameters": [{
                        "name": "the_constant",
                        "in": "path",
                        "required": true,
                        "schema": {
                            "type": "string",
                            "const": "MyConstantValue"
                        }
                    }],
                    "responses": {"200": {"description": "ok"}}
                }
            }
        }
    });

    let code = generate(spec);

    assert!(
        code.contains("pub enum SomeConstOperationTheConstant"),
        "expected SomeConstOperationTheConstant enum to be generated; got:\n{code}"
    );
    assert!(
        code.contains("MyConstantValue"),
        "expected MyConstantValue variant; got:\n{code}"
    );
    assert!(
        code.contains("the_constant : SomeConstOperationTheConstant"),
        "expected typed parameter, not impl AsRef<str>; got:\n{code}"
    );
    assert!(
        !code.contains("the_constant : impl AsRef < str >"),
        "parameter should NOT be impl AsRef<str>; got:\n{code}"
    );
}

#[test]
fn enum_path_parameter_generates_multi_variant_enum() {
    // The same code path also covers explicit enum-on-parameter, which was
    // also previously generated as `impl AsRef<str>`.
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "T", "version": "1.0.0"},
        "components": {"schemas": {"Stub": {"type": "object", "properties": {"id": {"type": "string"}}}}},
        "paths": {
            "/items/{kind}": {
                "get": {
                    "operationId": "getByKind",
                    "parameters": [{
                        "name": "kind",
                        "in": "path",
                        "required": true,
                        "schema": {
                            "type": "string",
                            "enum": ["alpha", "beta", "gamma"]
                        }
                    }],
                    "responses": {"200": {"description": "ok"}}
                }
            }
        }
    });

    let code = generate(spec);

    assert!(
        code.contains("pub enum GetByKindKind"),
        "expected GetByKindKind enum; got:\n{code}"
    );
    for variant in &["Alpha", "Beta", "Gamma"] {
        assert!(
            code.contains(variant),
            "expected variant {variant}; got:\n{code}"
        );
    }
    assert!(
        code.contains("kind : GetByKindKind"),
        "expected typed parameter; got:\n{code}"
    );
}

#[test]
fn const_only_query_parameter_generates_optional_enum() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "T", "version": "1.0.0"},
        "components": {"schemas": {"Stub": {"type": "object", "properties": {"id": {"type": "string"}}}}},
        "paths": {
            "/items": {
                "get": {
                    "operationId": "listItems",
                    "parameters": [{
                        "name": "version",
                        "in": "query",
                        "required": false,
                        "schema": {
                            "type": "string",
                            "const": "v2"
                        }
                    }],
                    "responses": {"200": {"description": "ok"}}
                }
            }
        }
    });

    let code = generate(spec);

    assert!(
        code.contains("pub enum ListItemsVersion"),
        "expected ListItemsVersion enum; got:\n{code}"
    );
    assert!(
        code.contains("version : Option < ListItemsVersion >"),
        "optional query param should be Option<Enum>; got:\n{code}"
    );
}

#[test]
fn const_parameter_emits_display_and_asref_impls() {
    // The path/query templating relies on Display (for format!) and AsRef<str>
    // for ergonomic use; verify both are emitted.
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "T", "version": "1.0.0"},
        "components": {"schemas": {"Stub": {"type": "object", "properties": {"id": {"type": "string"}}}}},
        "paths": {
            "/p/{x}": {
                "get": {
                    "operationId": "op",
                    "parameters": [{
                        "name": "x",
                        "in": "path",
                        "required": true,
                        "schema": {"type": "string", "const": "fixed"}
                    }],
                    "responses": {"200": {"description": "ok"}}
                }
            }
        }
    });

    let code = generate(spec);

    assert!(
        code.contains("impl std :: fmt :: Display for OpX"),
        "expected Display impl for the param enum; got:\n{code}"
    );
    assert!(
        code.contains("impl AsRef < str > for OpX"),
        "expected AsRef<str> impl for the param enum; got:\n{code}"
    );
}
