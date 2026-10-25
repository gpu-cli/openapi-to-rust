use openapi_to_rust::config::BuildersSection;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn spec() -> serde_json::Value {
    let mut document = json!({
        "openapi": "3.1.0", "info": {"title": "request content type", "version": "1"},
        "paths": {}
    });
    for (path, operation, required, media_type) in [
        ("/upload", "upload", false, "image/*"),
        ("/required", "required", true, "image/*"),
        ("/any", "any", true, "*/*"),
        ("/either", "either", true, "image/*"),
        ("/text", "text", false, "text/*"),
        ("/text-binary", "textBinary", true, "text/*"),
        ("/profile", "profile", false, "image/*; profile=v2"),
        ("/collision", "collision", false, "image/*"),
        ("/events", "events", false, "image/*"),
    ] {
        document["paths"][path] = json!({"post": {
            "operationId": operation,
            "requestBody": {"required": required, "content": {
                media_type: {"schema": {"type": "string", "format": "binary"}}
            }},
            "responses": {"200": {"description": "echo", "content": {
                "text/plain": {"schema": {"type": "string"}}
            }}}
        }});
    }
    document["paths"]["/either"]["post"]["requestBody"]["content"]["audio/*"] =
        json!({"schema": {"type": "string", "format": "binary"}});
    document["paths"]["/text"]["post"]["requestBody"]["content"]["text/*"]["schema"] =
        json!({"type": "string"});
    document["paths"]["/upload"]["post"]["parameters"] = json!([
        {"name": "content_type", "in": "query", "schema": {"type": "string"}},
        {"name": "content_type_2", "in": "query", "schema": {"type": "string"}},
        {"name": "__request_content_type_header", "in": "query", "schema": {"type": "string"}}
    ]);
    document["paths"]["/real"] = json!({"get": {
        "operationId": "collisionWithContentType", "responses": {"204": {"description": "ok"}}
    }});
    document["paths"]["/events"]["post"]["responses"] = json!({"200": {
        "description": "representations", "content": {
            "application/json": {"schema": {"type": "object", "required": ["ok"], "properties": {"ok": {"type": "boolean"}}}},
            "application/octet-stream": {"schema": {"type": "string", "format": "binary"}},
            "text/event-stream": {"schema": {"type": "string"}}
        }
    }});
    document
}

#[test]
fn generated_clients_send_concrete_request_media_types() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut dependencies = String::new();
    for (module, parsed_sse) in [("parsed", true), ("raw", false)] {
        let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
        let generator = CodeGenerator::new(GeneratorConfig {
            output_dir: temp.path().join("src").join(module),
            enable_async_client: true,
            enable_sse_client: parsed_sse,
            tracing_enabled: false,
            builders: BuildersSection {
                enabled: true,
                threshold: 0,
            },
            ..Default::default()
        });
        let generated = generator.generate_all_with_bindings(&mut analysis).unwrap();
        let method = generated
            .bindings
            .symbols
            .iter()
            .find(|symbol| symbol.path == "client::HttpClient::upload_with_content_type")
            .unwrap();
        assert_eq!(method.operation.as_ref().unwrap().operation_id, "upload");
        assert_eq!(
            method
                .signature
                .as_ref()
                .unwrap()
                .arguments
                .last()
                .unwrap()
                .name,
            "content_type_3"
        );
        generator.write_files(&generated.generation).unwrap();
        let emitted = std::fs::read_to_string(
            temp.path()
                .join("src")
                .join(module)
                .join("REQUIRED_DEPS.toml"),
        )
        .unwrap();
        assert!(emitted.contains("mime ="), "{emitted}");
        if parsed_sse {
            dependencies = emitted;
        }
    }
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"
[package]
name = "request-content-type-runtime"
version = "0.0.0"
edition = "2024"
publish = false
{dependencies}
[dev-dependencies]
axum = "0.8"
tokio = {{ version = "1", features = ["macros", "net", "rt-multi-thread"] }}
"#
        ),
    )
    .unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), RUNTIME).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/request-content-type-runtime"),
        )
        .env(
            "CARGO_BUILD_BUILD_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/request-content-type-runtime/build"),
        )
        .env_remove("CC_aarch64_apple_darwin")
        .env_remove("CXX_aarch64_apple_darwin")
        .env("CC", "cc")
        .env("CXX", "c++")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated runtime failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const RUNTIME: &str = r#"
#![allow(dead_code)]
mod parsed;
mod raw;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Bytes, http::{HeaderMap, Uri, StatusCode}, routing::{get, post}, response::IntoResponse};
    use futures_util::StreamExt;
    use serde_json::{Value, json};
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

    fn echo(headers: &HeaderMap, uri: &Uri, body: &Bytes) -> String {
        json!({
            "content_type": headers.get("content-type").map(|v| v.to_str().unwrap()),
            "count": headers.get_all("content-type").iter().count(),
            "content_length": headers.get("content-length").map(|v| v.to_str().unwrap()),
            "body": body.to_vec(), "query": uri.query()
        }).to_string()
    }
    fn decode(value: String) -> Value { serde_json::from_str(&value).unwrap() }

    #[tokio::test]
    async fn media_types_are_request_local_and_validated_before_send() {
        let requests = Arc::new(AtomicUsize::new(0));
        let counted = requests.clone();
        let app = Router::new()
            .route("/upload", post(move |headers: HeaderMap, uri: Uri, body: Bytes| {
                let requests = counted.clone();
                async move { requests.fetch_add(1, Ordering::SeqCst); echo(&headers, &uri, &body) }
            }))
            .route("/required", post(|headers: HeaderMap, uri: Uri, body: Bytes| async move { echo(&headers, &uri, &body) }))
            .route("/any", post(|headers: HeaderMap, uri: Uri, body: Bytes| async move { echo(&headers, &uri, &body) }))
            .route("/either", post(|headers: HeaderMap, uri: Uri, body: Bytes| async move { echo(&headers, &uri, &body) }))
            .route("/text", post(|headers: HeaderMap, uri: Uri, body: Bytes| async move { echo(&headers, &uri, &body) }))
            .route("/text-binary", post(|headers: HeaderMap, uri: Uri, body: Bytes| async move { echo(&headers, &uri, &body) }))
            .route("/profile", post(|headers: HeaderMap, uri: Uri, body: Bytes| async move { echo(&headers, &uri, &body) }))
            .route("/collision", post(|headers: HeaderMap, uri: Uri, body: Bytes| async move { echo(&headers, &uri, &body) }))
            .route("/real", get(|| async { StatusCode::NO_CONTENT }))
            .route("/events", post(|headers: HeaderMap, body: Bytes| async move {
                assert_eq!(headers["content-type"], "image/png");
                assert_eq!(&body[..], b"image");
                match headers.get("accept").and_then(|v| v.to_str().ok()).unwrap_or("") {
                    "text/event-stream" => ([("content-type", "text/event-stream")], "data: ok\n\n").into_response(),
                    "application/octet-stream" => ([("content-type", "application/octet-stream")], "binary").into_response(),
                    _ => axum::Json(json!({"ok": true})).into_response(),
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let transport = reqwest::Client::builder().https_only(false).no_proxy().build().unwrap();
        let client = parsed::HttpClient::with_client(transport.clone()).with_base_url(&url);

        let response = decode(client.upload_with_content_type(
            Some("query-type"), Some("query-two"), Some("local-name"), Some(vec![0, 255, 42]), "IMAGE/PNG; profile=avatar"
        ).await.unwrap());
        assert_eq!(response["content_type"], "IMAGE/PNG; profile=avatar");
        assert_eq!(response["count"], 1);
        assert_eq!(response["body"], json!([0, 255, 42]));
        let query = response["query"].as_str().unwrap();
        assert!(query.contains("content_type=query-type"), "{query}");
        assert!(query.contains("content_type_2=query-two"), "{query}");
        assert!(query.contains("__request_content_type_header=local-name"), "{query}");

        for content_type in ["image/jpeg", "image/png"] {
            let response = decode(client.upload_builder()
                .content_type(content_type)
                .with_content_type("query-value")
                .body(vec![1, 2]).send().await.unwrap());
            assert_eq!(response["content_type"], content_type);
            assert_eq!(response["count"], 1);
            assert_eq!(response["body"], json!([1, 2]));
            assert!(response["query"].as_str().unwrap().contains("content_type=query-value"));
        }
        let configured = client.clone().with_header("Content-Type", "invalid\r\nheader").with_header("content-type", "image/jpeg");
        let response = decode(configured.upload_builder().body(vec![]).content_type("image/png").send().await.unwrap());
        assert_eq!(response["content_type"], "image/png");
        assert_eq!(response["count"], 1);
        assert_eq!(response["body"], json!([]));

        let response = decode(client.upload_builder().content_type("invalid but ignored").send().await.unwrap());
        assert!(response["content_type"].is_null());
        assert_eq!(response["content_length"], "0");
        let response = decode(client.upload_with_content_type(None::<String>, None::<String>, None::<String>, None, "image/*").await.unwrap());
        assert!(response["content_type"].is_null());
        assert_eq!(response["content_length"], "0");

        let before = requests.load(Ordering::SeqCst);
        assert!(matches!(client.upload(None::<String>, None::<String>, None::<String>, Some(vec![])).await,
            Err(parsed::ApiOpError::Transport(parsed::HttpError::Config(_)))));
        assert!(matches!(client.upload_builder().body(vec![1]).send().await,
            Err(parsed::ApiOpError::Transport(parsed::HttpError::Config(_)))));
        for invalid in ["", "image/*", "*/*", "*/png", "image/*+json", "image/png, image/jpeg", "image", "image/png; broken", "text/plain", "image/png\r\nx: y"] {
            assert!(matches!(client.upload_builder().body(vec![1]).content_type(invalid).send().await,
                Err(parsed::ApiOpError::Transport(parsed::HttpError::Config(_)))), "{invalid}");
        }
        assert_eq!(requests.load(Ordering::SeqCst), before, "invalid requests must not reach transport");

        let response = decode(client.required_with_content_type(vec![4], "image/webp").await.unwrap());
        assert_eq!(response["content_type"], "image/webp");
        let response = decode(client.required_builder(vec![5]).content_type("image/png").send().await.unwrap());
        assert_eq!(response["body"], json!([5]));
        assert!(client.required(vec![1]).await.is_err());
        let response = decode(client.any_with_content_type(vec![6], "application/vnd.example+json; charset=utf-8").await.unwrap());
        assert_eq!(response["content_type"], "application/vnd.example+json; charset=utf-8");
        let response = decode(client.either_with_content_type(vec![9], "image/png").await.unwrap());
        assert_eq!(response["content_type"], "image/png");
        assert!(client.profile_with_content_type(Some(vec![1]), "image/png; profile=v3").await.is_err());
        let response = decode(client.profile_with_content_type(Some(vec![1]), "image/png; profile=v2").await.unwrap());
        assert_eq!(response["content_type"], "image/png; profile=v2");
        let response = decode(client.text_with_content_type(Some("hello".to_string()), "text/plain; charset=utf-8").await.unwrap());
        assert_eq!(response["content_type"], "text/plain; charset=utf-8");
        assert_eq!(response["body"], json!([104, 101, 108, 108, 111]));
        assert!(client.text(Some("missing type".to_string())).await.is_err());
        let response = decode(client.text_builder().body("hi".to_string()).content_type("text/custom").send().await.unwrap());
        assert_eq!(response["body"], json!([104, 105]));
        let response = decode(client.text_binary_with_content_type(vec![0, 255], "text/plain").await.unwrap());
        assert_eq!(response["body"], json!([0, 255]));

        client.collision_with_content_type().await.unwrap();
        let response = decode(client.collision_with_content_type_2(Some(vec![7]), "image/png").await.unwrap());
        assert_eq!(response["body"], json!([7]));
        let response = decode(client.collision_builder().body(vec![8]).content_type("image/png").send().await.unwrap());
        assert_eq!(response["body"], json!([8]));

        assert!(client.events_with_content_type(Some(b"image".to_vec()), "image/png").await.unwrap().ok);
        let events = client.events_sse_builder().body(b"image".to_vec()).content_type("image/png").send().await.unwrap();
        futures_util::pin_mut!(events);
        assert_eq!(events.next().await.unwrap().unwrap().data, "ok");
        assert!(events.next().await.is_none());
        assert_eq!(client.events_binary_with_content_type(Some(b"image".to_vec()), "image/png").await.unwrap(), "binary");
        let binary = client.events_binary_stream_builder().body(b"image".to_vec()).content_type("image/png").send().await.unwrap();
        futures_util::pin_mut!(binary);
        assert_eq!(binary.next().await.unwrap().unwrap(), "binary");
        let raw = raw::HttpClient::with_client(transport).with_base_url(&url);
        let events = raw.events_event_stream_with_content_type(Some(b"image".to_vec()), "image/png").await.unwrap();
        futures_util::pin_mut!(events);
        assert_eq!(events.next().await.unwrap().unwrap(), "data: ok\n\n");
        let events = raw.events_event_stream_builder().body(b"image".to_vec()).content_type("image/png").send().await.unwrap();
        futures_util::pin_mut!(events);
        assert_eq!(events.next().await.unwrap().unwrap(), "data: ok\n\n");
        server.abort();
    }
}
"#;
