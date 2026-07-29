use openapi_to_rust::streaming::{HttpMethod, StreamingConfig, StreamingEndpoint};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn live_sse_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "live SSE transport", "version": "1.0.0" },
        "paths": {
            "/events": {
                "post": {
                    "operationId": "streamEvents",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/StreamRequest" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "events",
                            "content": {
                                "text/event-stream": {
                                    "schema": { "$ref": "#/components/schemas/StreamEvent" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "StreamRequest": {
                    "type": "object",
                    "additionalProperties": true
                },
                "StreamEvent": {
                    "type": "object",
                    "additionalProperties": true
                }
            }
        }
    })
}

#[test]
#[ignore = "requires OPENAPI_TO_RUST_SSE_BASE_URL and a live OpenAI/Anthropic-compatible backend"]
fn generated_sse_transport_streams_openai_and_anthropic_protocols() {
    let base_url = std::env::var("OPENAPI_TO_RUST_SSE_BASE_URL")
        .expect("OPENAPI_TO_RUST_SSE_BASE_URL must point to the live backend");
    let model = std::env::var("OPENAPI_TO_RUST_SSE_MODEL")
        .unwrap_or_else(|_| "qwen/qwen3.5-9b".to_string());

    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let mut analysis = SchemaAnalyzer::new(live_sse_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        enable_async_client: false,
        enable_sse_client: true,
        tracing_enabled: false,
        streaming_config: Some(StreamingConfig {
            endpoints: vec![StreamingEndpoint {
                operation_id: "streamEvents".into(),
                path: "/events".into(),
                http_method: HttpMethod::Post,
                event_union_type: "StreamEvent".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    assert!(output_dir.join("sse.rs").is_file());
    let streaming = std::fs::read_to_string(output_dir.join("streaming.rs")).unwrap();
    assert!(streaming.contains("use super::sse::SseClient"));

    std::fs::write(
        temp.path().join("src/main.rs"),
        r#"mod generated;

use futures_util::StreamExt;
use generated::sse::SseClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_url = std::env::var("OPENAPI_TO_RUST_SSE_BASE_URL")?;
    let model = std::env::var("OPENAPI_TO_RUST_SSE_MODEL")?;
    let client = SseClient::new();

    let openai_request = client
        .post(format!("{base_url}/v1/chat/completions"))
        .bearer_auth("local-test")
        .json(&serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Reply with exactly: SSE works" }],
            "stream": true,
            "max_tokens": 16
        }));
    let mut openai = client.stream::<serde_json::Value>(openai_request).await?;
    let mut openai_chunks = 0_usize;
    while let Some(event) = openai.next().await {
        let event = event?;
        if event.get("object").and_then(serde_json::Value::as_str)
            == Some("chat.completion.chunk")
        {
            openai_chunks += 1;
        }
    }
    if openai_chunks == 0 {
        return Err("OpenAI stream produced no chat.completion.chunk events".into());
    }

    let anthropic_request = client
        .post(format!("{base_url}/v1/messages"))
        .header("x-api-key", "local-test")
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": model,
            "messages": [{ "role": "user", "content": "Reply with exactly: SSE works" }],
            "stream": true,
            "max_tokens": 16
        }));
    let mut anthropic = client.stream::<serde_json::Value>(anthropic_request).await?;
    let mut event_types = std::collections::BTreeSet::new();
    while let Some(event) = anthropic.next().await {
        let event = event?;
        if let Some(event_type) = event.get("type").and_then(serde_json::Value::as_str) {
            event_types.insert(event_type.to_string());
        }
    }
    for expected in ["message_start", "content_block_delta", "message_stop"] {
        if !event_types.contains(expected) {
            return Err(format!("Anthropic stream omitted {expected}; saw {event_types:?}").into());
        }
    }

    println!("OpenAI chunks: {openai_chunks}; Anthropic event types: {event_types:?}");
    Ok(())
}
"#,
    )
    .unwrap();

    let dependencies = std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "live-generated-sse-client"
version = "0.0.0"
edition = "2024"
publish = false

{dependencies}
tokio = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
"#
        ),
    )
    .unwrap();

    let output = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(temp.path())
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .env("CARGO_TARGET_DIR", temp.path().join("cargo-target"))
        .env("OPENAPI_TO_RUST_SSE_BASE_URL", base_url)
        .env("OPENAPI_TO_RUST_SSE_MODEL", model)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "live generated SSE client failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("{}", String::from_utf8_lossy(&output.stdout));
}
