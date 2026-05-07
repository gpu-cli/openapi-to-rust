//! End-to-end test for the multi-response error envelope work in issue #8.
//!
//! These tests generate a complete client (types + http_client) for a spec
//! that exercises the new code paths — multiple responses with different
//! schemas, declared error bodies, and ops with no error bodies — and then
//! invoke `cargo check` on the generated crate to verify the emitted code is
//! not just syntactically valid Rust but actually compiles against the
//! reqwest / serde / thiserror runtime it depends on.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const CLIENT_CARGO_TOML: &str = r#"
[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
reqwest = { version = "0.12", features = ["json", "multipart"] }
reqwest-middleware = { version = "0.4", features = ["multipart"] }
thiserror = "1.0"
tokio = { version = "1.0", features = ["full"] }
validator = { version = "0.20", features = ["derive"] }
"#;

/// Generate types + http_client for a spec, write a compilable crate to a
/// temp dir, run `cargo check`, and panic if it fails. Returns the temp dir
/// so callers can keep it alive long enough for inspection on failure.
fn assert_generates_compiling_client(name: &str, spec: serde_json::Value) -> TempDir {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analyze");

    let config = GeneratorConfig {
        module_name: name.to_string(),
        enable_async_client: true,
        tracing_enabled: false,
        ..Default::default()
    };
    let generator = CodeGenerator::new(config);
    let types_code = generator.generate(&mut analysis).expect("generate types");
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("generate http client");

    let temp = TempDir::new().expect("temp dir");
    let temp_path = temp.path();

    fs::write(
        temp_path.join("Cargo.toml"),
        CLIENT_CARGO_TOML.replace("{name}", name),
    )
    .expect("write Cargo.toml");

    let src = temp_path.join("src");
    fs::create_dir_all(&src).expect("create src");
    fs::write(src.join("types.rs"), types_code).expect("write types.rs");
    fs::write(src.join("client.rs"), client_code).expect("write client.rs");
    fs::write(src.join("lib.rs"), "pub mod types;\npub mod client;\n").expect("write lib.rs");

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(temp_path)
        .output()
        .expect("run cargo check");

    if !output.status.success() {
        eprintln!(
            "STDOUT:\n{}\n\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Generated client for `{name}` failed to compile");
    }
    temp
}

fn multi_response_spec() -> serde_json::Value {
    serde_json::json!({
        "openapi": "3.1.0",
        "info": { "title": "Multi-Response Test", "version": "1.0.0" },
        "paths": {
            "/todos": {
                "get": {
                    "operationId": "listTodos",
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": { "$ref": "#/components/schemas/Todo" }
                                    }
                                }
                            }
                        },
                        "400": {
                            "description": "Bad Request",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/BadRequest" }
                                }
                            }
                        }
                    }
                }
            },
            "/ping": {
                "get": {
                    "operationId": "ping",
                    "responses": {
                        "200": {
                            "description": "OK",
                            "content": {
                                "application/json": {
                                    "schema": { "type": "string" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Todo": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "title": { "type": "string" }
                    },
                    "required": ["id", "title"]
                },
                "BadRequest": {
                    "type": "object",
                    "properties": {
                        "error": { "type": "string" }
                    },
                    "required": ["error"]
                }
            }
        }
    })
}

#[test]
fn test_generated_multi_response_client_compiles() {
    let _temp = assert_generates_compiling_client("multi_response_client", multi_response_spec());
}

/// The anthropic and openai fixtures are realistic-shape API specs whose
/// generated clients must compile end-to-end. These regression tests catch
/// codegen breakage in non-trivial schema territory (oneOfs, discriminated
/// unions, etc) that the toy multi_response_spec doesn't exercise.
#[test]
fn test_generated_anthropic_client_compiles() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/anthropic.yml");
    let raw = fs::read_to_string(&path).expect("read anthropic fixture");
    let spec: serde_json::Value =
        serde_yaml::from_str(&raw).expect("parse anthropic fixture");
    let _temp = assert_generates_compiling_client("anthropic_client", spec);
}

#[test]
fn test_generated_openai_client_compiles() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/openai-responses.json");
    let raw = fs::read_to_string(&path).expect("read openai fixture");
    let spec: serde_json::Value =
        serde_json::from_str(&raw).expect("parse openai fixture");
    let _temp = assert_generates_compiling_client("openai_client", spec);
}

#[test]
fn test_generated_client_emits_per_op_error_enum() {
    let spec = multi_response_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer");
    let analysis = analyzer.analyze().expect("analyze");

    let config = GeneratorConfig {
        module_name: "multi_response_client".to_string(),
        enable_async_client: true,
        ..Default::default()
    };
    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("generate http client");

    // The op with a 400 schema should get its own typed error enum.
    assert!(
        client_code.contains("pub enum ListTodosApiError"),
        "Expected ListTodosApiError enum in generated client. Code:\n{client_code}"
    );
    assert!(
        client_code.contains("Status400(BadRequest)"),
        "Expected Status400(BadRequest) variant. Code:\n{client_code}"
    );

    // The signature should plug the per-op error type into ApiOpError.
    assert!(
        client_code.contains("ApiOpError<ListTodosApiError>"),
        "list_todos signature should use ApiOpError<ListTodosApiError>. Code:\n{client_code}"
    );

    // The op without any non-2xx schemas should fall back to serde_json::Value.
    assert!(
        client_code.contains("ApiOpError<serde_json::Value>"),
        "ping signature should use ApiOpError<serde_json::Value>. Code:\n{client_code}"
    );
}
