//! Regression gate: generated clients must compile for `wasm32-unknown-unknown`.
//!
//! The generated HTTP client buffers response bodies through
//! `__read_bounded_response_body`. It originally used `reqwest::Response::chunk()`,
//! which is native-only: reqwest's wasm backend exposes `json`/`text`/`bytes`/
//! `bytes_stream` but not `chunk`. Every generated client therefore failed to
//! build under `trunk serve` with `no method named chunk found for struct
//! Response` (issue #74).
//!
//! This test generates a client that exercises the buffered success, buffered
//! error, binary, and auto-detected SSE paths, plus the two opt-in stacks that
//! needed target-scoped handling:
//!
//! * retry (`[http_client.retry]`) pulls `getrandom 0.4`, which rejects wasm32
//!   unless the `wasm_js` feature is enabled; the emitted fragment now scopes
//!   that feature to `cfg(target_arch = "wasm32")`.
//! * the SSE runtime (`enable_sse_client` + `[[streaming.endpoints]]`) emits a
//!   cfg-split `BoxSseStream<T>` alias and `#[cfg_attr(...)] async_trait`, so
//!   native keeps `Send` streams and wasm drops the bound.
//!
//! All three then `cargo check` for wasm32 (and native) from the exact emitted
//! `REQUIRED_DEPS.toml`. This is the only automated check that would have caught
//! the original regression, so it belongs on CI.
//!
//! The `wasm32-unknown-unknown` target must be installed. It skips (with a
//! printed notice) when it is absent so it cannot fail a machine that simply
//! lacks the target; CI installs it explicitly.

use openapi_to_rust::http_config::HttpClientConfig;
use openapi_to_rust::streaming::{HttpMethod, StreamingConfig, StreamingEndpoint};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, RetryConfig, SchemaAnalyzer};
use serde_json::json;
use std::collections::HashMap;
use std::process::Command;

/// A spec touching every place the bounded buffering helper is emitted:
/// buffered JSON, buffered text, buffered binary, an error body, and an
/// auto-detected `text/event-stream` response, plus concrete media-type
/// validation for wildcard request bodies.
fn wasm_client_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "wasm client", "version": "1.0.0" },
        "paths": {
            "/upload": { "post": {
                "operationId": "uploadImage",
                "requestBody": { "required": true, "content": {
                    "image/*": { "schema": { "type": "string", "format": "binary" } }
                }},
                "responses": { "204": { "description": "uploaded" } }
            }},
            "/json": { "get": {
                "operationId": "getJson",
                "responses": {
                    "200": { "description": "ok", "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/Thing" }
                    }}},
                    "400": { "description": "err", "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/Thing" }
                    }}}
                }
            }},
            "/text": { "get": {
                "operationId": "getText",
                "responses": {
                    "200": { "description": "ok", "content": { "text/plain": {
                        "schema": { "type": "string" }
                    }}}
                }
            }},
            "/binary": { "get": {
                "operationId": "getBinary",
                "responses": {
                    "200": { "description": "ok", "content": { "application/octet-stream": {
                        "schema": { "type": "string", "format": "binary" }
                    }}}
                }
            }},
            "/events": { "get": {
                "operationId": "streamEvents",
                "responses": {
                    "200": { "description": "events", "content": { "text/event-stream": {
                        "schema": { "$ref": "#/components/schemas/Thing" }
                    }}}
                }
            }}
        },
        "components": { "schemas": {
            "Thing": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" }, "name": { "type": "string" } }
            }
        }}
    })
}

/// True when the wasm32 standard library is present in the active sysroot.
/// `rustup target list --installed` is the authoritative source, but this
/// avoids assuming the toolchain is rustup-managed.
fn wasm32_target_installed() -> bool {
    let output = Command::new("rustc")
        .args([
            "--print",
            "target-libdir",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    let libdir = String::from_utf8_lossy(&output.stdout);
    // A missing target yields the path but not the `lib` directory.
    std::path::Path::new(libdir.trim()).is_dir()
}

/// One generated client configuration to compile-check.
struct Case {
    label: &'static str,
    enable_sse_client: bool,
    retry_config: Option<RetryConfig>,
}

#[test]
fn generated_client_compiles_for_wasm32() {
    if !wasm32_target_installed() {
        eprintln!(
            "skipping: wasm32-unknown-unknown target not installed \
             (run `rustup target add wasm32-unknown-unknown`)"
        );
        return;
    }

    // `tracing_enabled` is true by default and pulls in reqwest-tracing, so the
    // real default stack is what gets checked first.
    let cases = [
        Case {
            label: "default",
            enable_sse_client: false,
            retry_config: None,
        },
        Case {
            label: "retry",
            enable_sse_client: false,
            retry_config: Some(RetryConfig {
                max_retries: 2,
                initial_delay_ms: 100,
                max_delay_ms: 1_000,
            }),
        },
        Case {
            label: "sse",
            enable_sse_client: true,
            retry_config: None,
        },
        Case {
            label: "sse-retry",
            enable_sse_client: true,
            retry_config: Some(RetryConfig {
                max_retries: 2,
                initial_delay_ms: 100,
                max_delay_ms: 1_000,
            }),
        },
    ];

    for case in &cases {
        check_case(case);
    }
}

fn check_case(case: &Case) {
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");

    let mut analysis = SchemaAnalyzer::new(wasm_client_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let mut config = GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "generated".into(),
        enable_async_client: true,
        enable_sse_client: case.enable_sse_client,
        tracing_enabled: true,
        retry_config: case.retry_config.clone(),
        http_client_config: Some(HttpClientConfig {
            base_url: None,
            timeout_seconds: None,
            max_response_body_bytes: Some(8),
            default_headers: HashMap::new(),
        }),
        ..Default::default()
    };
    if case.enable_sse_client {
        config.streaming_config = Some(StreamingConfig {
            endpoints: vec![StreamingEndpoint {
                operation_id: "streamEvents".into(),
                path: "/events".into(),
                http_method: HttpMethod::Get,
                event_union_type: "Thing".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    // The generated client module is mounted at src/generated; the crate root
    // is just a re-export shim. `dead_code` is expected in a spec-covering
    // compile check.
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "#![allow(dead_code, unused_imports)]\npub mod generated;\n",
    )
    .unwrap();

    let dependencies = std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml"))
        .expect("generated REQUIRED_DEPS.toml");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[workspace]\n\n\
             [package]\n\
             name = \"wasm-client-check\"\n\
             version = \"0.0.0\"\n\
             edition = \"2024\"\n\
             publish = false\n\n\
             {dependencies}"
        ),
    )
    .unwrap();

    // Reuse one workspace target dir across cases so reqwest's wasm backend is
    // compiled once. The scratch crate has no lock file, so allow the resolver
    // to reach the index on a cold cache rather than passing `--offline`.
    let manifest_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/generated-wasm32-client");
    for target in ["wasm32-unknown-unknown", "native"] {
        let mut command = Command::new("cargo");
        command.args(["check", "--lib", "--quiet"]);
        if target == "wasm32-unknown-unknown" {
            command.args(["--target", target]);
        }
        let output = command
            .current_dir(temp.path())
            .env("CARGO_TARGET_DIR", &manifest_dir)
            .output()
            .expect("cargo check runs");
        assert!(
            output.status.success(),
            "generated client ({}) failed to compile for {target} (issue #74):\n{}",
            case.label,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
