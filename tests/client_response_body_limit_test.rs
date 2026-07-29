use openapi_to_rust::http_config::HttpClientConfig;
use openapi_to_rust::streaming::{HttpMethod, StreamingConfig, StreamingEndpoint};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::collections::HashMap;
use std::process::Command;

fn response_limit_spec() -> serde_json::Value {
    let success = |content_type: &str, schema: serde_json::Value| {
        json!({
            "200": {
                "description": "success",
                "content": { (content_type): { "schema": schema } }
            }
        })
    };
    json!({
        "openapi": "3.1.0",
        "info": { "title": "response limits", "version": "1.0.0" },
        "paths": {
            "/oversized-json": { "get": {
                "operationId": "getOversizedJson",
                "responses": success("application/json", json!({ "type": "string" }))
            }},
            "/oversized-text": { "get": {
                "operationId": "getOversizedText",
                "responses": success("text/plain", json!({ "type": "string" }))
            }},
            "/oversized-binary": { "get": {
                "operationId": "getOversizedBinary",
                "responses": success("application/octet-stream", json!({
                    "type": "string", "format": "binary"
                }))
            }},
            "/oversized-error": { "get": {
                "operationId": "getOversizedError",
                "responses": {
                    "200": {
                        "description": "success",
                        "content": { "text/plain": { "schema": { "type": "string" } } }
                    },
                    "400": {
                        "description": "error",
                        "content": { "application/json": { "schema": {
                            "$ref": "#/components/schemas/ErrorBody"
                        } } }
                    }
                }
            }},
            "/ok-json": { "get": {
                "operationId": "getOkJson",
                "responses": success("application/json", json!({ "type": "string" }))
            }},
            "/ok-text": { "get": {
                "operationId": "getOkText",
                "responses": success("text/plain", json!({ "type": "string" }))
            }},
            "/ok-binary": { "get": {
                "operationId": "getOkBinary",
                "responses": success("application/octet-stream", json!({
                    "type": "string", "format": "binary"
                }))
            }},
            "/oversized-sse-error": { "get": {
                "operationId": "streamEvents",
                "responses": {
                    "200": {
                        "description": "events",
                        "content": { "text/event-stream": { "schema": {
                            "$ref": "#/components/schemas/StreamEvent"
                        } } }
                    }
                }
            }}
        },
        "components": { "schemas": {
            "ErrorBody": {
                "type": "object",
                "required": ["message"],
                "properties": { "message": { "type": "string" } }
            },
            "StreamEvent": {
                "type": "object",
                "required": ["message"],
                "properties": { "message": { "type": "string" } }
            }
        }}
    })
}

#[test]
fn generated_clients_bound_chunked_responses_without_content_length() {
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let mut analysis = SchemaAnalyzer::new(response_limit_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        enable_async_client: true,
        enable_sse_client: true,
        tracing_enabled: false,
        http_client_config: Some(HttpClientConfig {
            base_url: None,
            timeout_seconds: None,
            max_response_body_bytes: Some(32),
            default_headers: HashMap::new(),
        }),
        streaming_config: Some(StreamingConfig {
            endpoints: vec![StreamingEndpoint {
                operation_id: "streamEvents".into(),
                path: "/oversized-sse-error".into(),
                http_method: HttpMethod::Get,
                event_union_type: "StreamEvent".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    let client = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap();
    assert!(client.content.contains("DEFAULT_MAX_RESPONSE_BODY_BYTES"));
    assert!(client.content.contains("with_max_response_body_bytes"));
    assert!(client.content.contains("checked_add(chunk.len())"));
    assert!(!client.content.contains("response.bytes().await"));

    let streaming = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("streaming.rs"))
        .unwrap();
    assert!(streaming.content.contains("with_max_response_body_bytes"));
    let sse = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("sse.rs"))
        .unwrap();
    assert!(sse.content.contains("ResponseTooLarge"));
    assert!(!sse.content.contains("response.text().await"));

    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::client::{ApiOpError, HttpClient, HttpError};
    use super::generated::streaming::{
        StreamEventsStreamingClient, StreamingClient, StreamingError,
    };
    use super::generated::sse::{SseClient, SseReconnectOptions};
    use futures_util::StreamExt;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn spawn_chunked_server() -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..10 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0_u8; 1024];
                    let read = socket.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request);
                let path = request.split_whitespace().nth(1).unwrap();
                let (status, content_type, chunks): (&str, &str, Vec<&[u8]>) = match path {
                    "/ok-json" => ("200 OK", "application/json", vec![b"\"ok\""]),
                    "/ok-text" => ("200 OK", "text/plain", vec![b"hello"]),
                    "/ok-binary" => (
                        "200 OK",
                        "application/octet-stream",
                        vec![&[0, 1, 2, 3, 4]],
                    ),
                    "/oversized-error" => (
                        "400 Bad Request",
                        "application/json",
                        vec![b"12345", b"67890"],
                    ),
                    "/oversized-sse-error" => (
                        "500 Internal Server Error",
                        "application/json",
                        vec![b"12345", b"67890"],
                    ),
                    "/sse-chunks" => (
                        "200 OK",
                        "text/event-stream; charset=utf-8",
                        vec![
                            b": keepalive\r",
                            b"\nevent: ping\r\ndata: {\"type\":\"ping\"}\r\n\r\n",
                            b"event: message\ndata: {\"message\":\"fir",
                            b"st\"}\n\ndata: [DONE]\n\n",
                            b"data: {\"message\":\"ignored\"}\n\n",
                        ],
                    ),
                    "/oversized-text" => ("200 OK", "text/plain", vec![b"12345", b"67890"]),
                    "/oversized-binary" => (
                        "200 OK",
                        "application/octet-stream",
                        vec![b"12345", b"67890"],
                    ),
                    _ => ("200 OK", "application/json", vec![b"12345", b"67890"]),
                };
                let headers = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
                );
                socket.write_all(headers.as_bytes()).await.unwrap();
                for chunk in chunks {
                    socket
                        .write_all(format!("{:X}\r\n", chunk.len()).as_bytes())
                        .await
                        .unwrap();
                    socket.write_all(chunk).await.unwrap();
                    socket.write_all(b"\r\n").await.unwrap();
                }
                socket.write_all(b"0\r\n\r\n").await.unwrap();
            }
        });
        address
    }

    async fn spawn_reconnect_server() -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let mut buffer = [0_u8; 1024];
                    let read = socket.read(&mut buffer).await.unwrap();
                    request.extend_from_slice(&buffer[..read]);
                    if read == 0 || request.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
                if attempt == 0 {
                    assert!(!request.contains("last-event-id:"));
                } else {
                    assert!(request.contains("last-event-id: 42"), "{request}");
                }

                let body = if attempt == 0 {
                    b"id: 42\nretry: 1\nevent: update\ndata: {\"sequence\":1}\n\n".as_slice()
                } else {
                    b"id: 43\nevent: update\ndata: {\"sequence\":2}\n\ndata: [DONE]\n\n".as_slice()
                };
                let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                socket.write_all(headers.as_bytes()).await.unwrap();
                socket
                    .write_all(format!("{:X}\r\n", body.len()).as_bytes())
                    .await
                    .unwrap();
                socket.write_all(body).await.unwrap();
                socket.write_all(b"\r\n0\r\n\r\n").await.unwrap();
            }
        });
        (address, task)
    }

    macro_rules! assert_too_large {
        ($future:expr) => {
            match $future.await.unwrap_err() {
                ApiOpError::Transport(HttpError::ResponseTooLarge { limit }) => {
                    assert_eq!(limit, 8)
                }
                other => panic!("unexpected error: {other:?}"),
            }
        };
    }

    #[tokio::test]
    async fn cumulative_chunk_limit_applies_to_every_buffered_response() {
        let address = spawn_chunked_server().await;
        let client = HttpClient::new()
            .with_base_url(format!("http://{address}"))
            .with_max_response_body_bytes(8);

        assert_too_large!(client.get_oversized_json());
        assert_too_large!(client.get_oversized_text());
        assert_too_large!(client.get_oversized_binary());
        assert_too_large!(client.get_oversized_error());
        assert_eq!(client.get_ok_json().await.unwrap(), "ok");
        assert_eq!(client.get_ok_text().await.unwrap(), "hello");
        assert_eq!(client.get_ok_binary().await.unwrap().as_ref(), &[0, 1, 2, 3, 4]);

        let streaming_client = StreamingClient::new()
            .with_base_url(format!("http://{address}"))
            .with_max_response_body_bytes(8);
        let mut stream = streaming_client.stream_stream_events().await.unwrap();
        match stream.next().await.unwrap().unwrap_err() {
            StreamingError::ResponseTooLarge { limit } => assert_eq!(limit, 8),
            other => panic!("unexpected streaming error: {other:?}"),
        }

        let sse_client = SseClient::new();
        let request = sse_client.get(format!("http://{address}/sse-chunks"));
        let mut stream = sse_client
            .stream::<serde_json::Value>(request)
            .await
            .unwrap();
        assert_eq!(
            stream.next().await.unwrap().unwrap(),
            serde_json::json!({ "message": "first" })
        );
        assert!(stream.next().await.is_none());

        let request = sse_client.get(format!("http://{address}/sse-chunks"));
        let mut raw = sse_client.stream_raw(request).await.unwrap();
        assert_eq!(raw.next().await.unwrap().unwrap().event, "ping");
        assert_eq!(raw.next().await.unwrap().unwrap().event, "message");
        assert_eq!(raw.next().await.unwrap().unwrap().data, "[DONE]");
        assert!(raw.next().await.is_none());

        let (reconnect_address, reconnect_task) = spawn_reconnect_server().await;
        let reconnecting = SseClient::new().with_reconnect_options(SseReconnectOptions {
            max_retries: 1,
            initial_retry_delay: Duration::ZERO,
            max_retry_delay: Duration::from_millis(10),
            backoff_multiplier: 1.0,
        });
        let request = reconnecting.get(format!("http://{reconnect_address}/events"));
        let mut events = reconnecting
            .stream_json_reconnecting::<serde_json::Value>(request)
            .await
            .unwrap();
        let first = events.next().await.unwrap().unwrap();
        assert_eq!(first.event, "update");
        assert_eq!(first.id.as_deref(), Some("42"));
        assert_eq!(first.retry, Some(Duration::from_millis(1)));
        assert_eq!(first.data, serde_json::json!({ "sequence": 1 }));
        let second = events.next().await.unwrap().unwrap();
        assert_eq!(second.id.as_deref(), Some("43"));
        assert_eq!(second.data, serde_json::json!({ "sequence": 2 }));
        assert!(events.next().await.is_none());
        reconnect_task.await.unwrap();
    }
}
"#,
    )
    .unwrap();

    let dependencies = std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "bounded-response-client"
version = "0.0.0"
edition = "2024"
publish = false

{dependencies}
tokio = {{ version = "1", features = ["io-util", "macros", "net", "rt-multi-thread"] }}
"#
        ),
    )
    .unwrap();

    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .env("CARGO_TARGET_DIR", temp.path().join("cargo-target"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated client runtime failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
