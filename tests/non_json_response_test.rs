use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn non_json_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "non-json responses", "version": "1.0.0" },
        "components": { "schemas": {
            "Reply": {
                "type": "object",
                "required": ["value"],
                "properties": { "value": { "type": "string" } }
            }
        }},
        "paths": {
            "/text": { "get": {
                "operationId": "getText",
                "responses": { "200": { "description": "text", "content": {
                    "text/plain": { "schema": { "type": "string" } }
                }}}
            }},
            "/binary": { "get": {
                "operationId": "getBinary",
                "responses": { "200": { "description": "binary", "content": {
                    "application/octet-stream": {
                        "schema": { "type": "string", "format": "binary" }
                    }
                }}}
            }},
            "/negotiated": { "get": {
                "operationId": "getNegotiated",
                "responses": { "200": { "description": "negotiated", "content": {
                    "text/plain": { "schema": { "type": "string" } },
                    "application/zip": {
                        "schema": { "type": "string", "format": "binary" }
                    }
                }}}
            }},
            "/mixed-status": { "get": {
                "operationId": "getMixedStatus",
                "responses": {
                    "200": { "description": "text", "content": {
                        "text/plain": { "schema": { "type": "string" } }
                    }},
                    "201": { "description": "binary", "content": {
                        "application/octet-stream": {
                            "schema": { "type": "string", "format": "binary" }
                        }
                    }}
                }
            }},
            "/created-body": { "get": {
                "operationId": "getCreatedBody",
                "responses": {
                    "200": { "description": "accepted without a body" },
                    "201": { "description": "created text", "content": {
                        "text/plain": { "schema": { "type": "string" } }
                    }}
                }
            }},
            "/same-json": { "get": {
                "operationId": "getSameJson",
                "responses": {
                    "200": { "description": "ok", "content": {
                        "application/json": { "schema": { "$ref": "#/components/schemas/Reply" } }
                    }},
                    "201": { "description": "created", "content": {
                        "application/json": { "schema": { "$ref": "#/components/schemas/Reply" } }
                    }}
                }
            }}
        }
    })
}

#[test]
fn generated_client_returns_text_and_preserves_invalid_utf8_binary() {
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let mut analysis = SchemaAnalyzer::new(non_json_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    let client = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .expect("generated client.rs");
    assert!(
        client
            .content
            .contains("pub async fn get_text(&self) -> Result<String")
    );
    assert!(
        client.content.contains("pub async fn get_binary")
            && client.content.contains("Result<bytes::Bytes"),
        "{}",
        client.content
    );
    assert!(
        client.content.contains("pub async fn get_created_body")
            && client.content.contains("-> Result<String"),
        "{}",
        client.content
    );
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::client::{ApiOpError, HttpClient};
    use axum::{body::Bytes, http::{header::ACCEPT, HeaderMap, StatusCode}, routing::get, Router};

    #[tokio::test]
    async fn response_bytes_are_not_utf8_decoded() {
        let app = Router::new()
            .route("/text", get(|| async { "hello" }))
            .route("/binary", get(|| async {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                    Bytes::from_static(&[0, 159, 146, 150, 255]),
                )
            }))
            .route("/negotiated", get(|headers: HeaderMap| async move {
                headers
                    .get(ACCEPT)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing")
                    .to_string()
            }))
            .route("/mixed-status", get(|| async {
                (
                    StatusCode::CREATED,
                    [(axum::http::header::CONTENT_TYPE, "application/octet-stream")],
                    Bytes::from_static(&[0, 159, 146, 150, 255]),
                )
            }))
            .route("/created-body", get(|| async {
                (StatusCode::CREATED, "created")
            }))
            .route("/same-json", get(|| async {
                (
                    StatusCode::CREATED,
                    axum::Json(serde_json::json!({ "value": "same" })),
                )
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = HttpClient::new()
            .with_base_url(format!("http://{address}"))
            .with_header("accept", "application/zip");
        assert_eq!(client.get_text().await.unwrap(), "hello");
        assert_eq!(
            client.get_binary().await.unwrap().as_ref(),
            &[0, 159, 146, 150, 255],
        );
        assert_eq!(client.get_negotiated().await.unwrap(), "text/plain");
        assert_eq!(client.get_created_body().await.unwrap(), "created");
        assert_eq!(client.get_same_json().await.unwrap().value, "same");

        let error = client.get_mixed_status().await.unwrap_err();
        match error {
            ApiOpError::Api(error) => {
                assert_eq!(error.status, 201);
                assert_eq!(error.raw_body, vec![0, 159, 146, 150, 255]);
                assert!(error.body.contains('\u{fffd}'));
                assert!(error.parse_error.as_deref().unwrap().contains("unexpected successful status"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
"#,
    )
    .unwrap();
    let dependencies = std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml")).unwrap();
    assert!(dependencies.contains("bytes ="), "{dependencies}");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "non-json-response-client"
version = "0.0.0"
edition = "2024"
publish = false

{dependencies}
axum = "0.8"
tokio = {{ version = "1", features = ["macros", "net", "rt-multi-thread"] }}
"#
        ),
    )
    .unwrap();

    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env(
            "CARGO_BUILD_BUILD_DIR",
            "/private/tmp/non-json-media-target/client-runtime-build",
        )
        .env(
            "CARGO_TARGET_DIR",
            "/private/tmp/non-json-media-target/client-runtime-target",
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated client runtime failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
