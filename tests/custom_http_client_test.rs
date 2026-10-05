use openapi_to_rust::http_config::HttpClientConfig;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, RetryConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

#[test]
fn custom_client_preserves_transport_and_constructor_compatibility() {
    let temp = tempfile::TempDir::new().unwrap();
    let spec = json!({
        "openapi": "3.1.0", "info": {"title": "custom client", "version": "1"},
        "paths": {
            "/text": {"get": {"operationId": "text", "responses": {
                "200": {"description": "ok", "content": {"text/plain": {"schema": {"type": "string"}}}}
            }}},
            "/events": {"get": {"operationId": "events", "responses": {
                "200": {"description": "ok", "content": {
                    "application/json": {"schema": {"type": "string"}},
                    "text/event-stream": {"schema": {"type": "string"}}
                }}
            }}},
            "/collision": {"get": {"operationId": "withClient", "responses": {"204": {"description": "ok"}}}},
            "/config-collision": {"get": {"operationId": "withClientAndConfig", "responses": {"204": {"description": "ok"}}}}
        }
    });
    let mut modules = String::new();
    let mut checks = String::new();
    let mut dependencies = String::new();
    for (name, retry, tracing, args) in [
        ("plain", false, false, ""),
        ("retry", true, false, "None"),
        ("trace_only", false, true, "false"),
        ("both", true, true, "None, false"),
    ] {
        let mut analysis = SchemaAnalyzer::new(spec.clone())
            .unwrap()
            .analyze()
            .unwrap();
        let generator = CodeGenerator::new(GeneratorConfig {
            output_dir: temp.path().join("src").join(name),
            enable_async_client: true,
            enable_sse_client: true,
            retry_config: retry.then_some(RetryConfig {
                max_retries: 1,
                initial_delay_ms: 1,
                max_delay_ms: 2,
            }),
            tracing_enabled: tracing,
            http_client_config: Some(HttpClientConfig {
                base_url: Some("http://configured.invalid".into()),
                max_response_body_bytes: Some(32),
                timeout_seconds: None,
                default_headers: Default::default(),
            }),
            ..Default::default()
        });
        let result = generator.generate_all(&mut analysis).unwrap();
        generator.write_files(&result).unwrap();
        let client = result
            .files
            .iter()
            .find(|file| file.path.to_str() == Some("client.rs"))
            .unwrap();
        assert!(client.content.contains("http://configured.invalid"));
        if name == "both" {
            dependencies =
                std::fs::read_to_string(temp.path().join("src/both/REQUIRED_DEPS.toml")).unwrap();
        }
        modules.push_str(&format!("mod {name};\n"));
        let explicit = if retry || tracing {
            format!("{name}::client::HttpClient::with_client_and_config(transport.clone(), {args})")
        } else {
            format!("{name}::client::HttpClient::with_client(transport.clone())")
        };
        checks.push_str(&format!(r#"
        {{
            use {name}::client::HttpClient;
            let _ = HttpClient::new();
            let _ = HttpClient::default();
            let supplied = HttpClient::with_client(transport.clone());
            for client in [supplied, {explicit}] {{
                let client = client.with_base_url(&base_url);
                assert_eq!(client.text().await.unwrap(), "custom");
                let mut events = client.events_sse().await.unwrap();
                assert_eq!(events.next().await.unwrap().unwrap().data, "custom");
                assert!(events.next().await.is_none());
                assert!(matches!(client.clone().with_base_url(format!("{{base_url}}/large")).text().await,
                    Err({name}::client::ApiOpError::Transport({name}::client::HttpError::ResponseTooLarge {{ limit: 32 }}))));
                client.with_client_2().await.unwrap();
                client.with_client_and_config_2().await.unwrap();
            }}
        }}
        "#));
        if retry {
            let tracing_arg = if tracing { ", false" } else { "" };
            checks.push_str(&format!(r#"
        {{
            use {name}::client::{{HttpClient, RetryConfig}};
            attempts.store(0, std::sync::atomic::Ordering::SeqCst);
            let client = HttpClient::with_client(transport.clone()).with_base_url(format!("{{base_url}}/retry"));
            assert!(client.text().await.is_err());
            assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1, "retry must be disabled by default");
            attempts.store(0, std::sync::atomic::Ordering::SeqCst);
            let client = HttpClient::with_client_and_config(transport.clone(), Some(RetryConfig {{
                max_retries: 1, initial_delay_ms: 1, max_delay_ms: 2,
            }}){tracing_arg}).with_base_url(format!("{{base_url}}/retry"));
            assert_eq!(client.text().await.unwrap(), "custom");
            assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
        }}
        "#));
        }
        if retry || tracing {
            checks.push_str(&format!(
                "let _ = {name}::client::HttpClient::with_config({args});\n"
            ));
        }
    }
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"
[package]
name = "generated-custom-http-client"
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
    std::fs::write(temp.path().join("src/lib.rs"), format!(r#"
#![allow(dead_code)]
{modules}
#[cfg(test)]
mod tests {{
    use super::*;
    use axum::{{Router, routing::get, http::{{HeaderMap, StatusCode}}}};
    use futures_util::StreamExt;

    async fn text(headers: HeaderMap) -> (StatusCode, &'static str) {{
        if headers.get("x-transport").and_then(|h| h.to_str().ok()) == Some("custom") {{
            (StatusCode::OK, "custom")
        }} else {{ (StatusCode::UNAUTHORIZED, "missing custom transport") }}
    }}

    #[tokio::test]
    async fn supplied_transport_reaches_http_and_sse() {{
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let retry_attempts = attempts.clone();
        let app = Router::new()
            .route("/retry/text", get(move |headers: HeaderMap| {{
                let attempts = retry_attempts.clone();
                async move {{
                    if attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {{
                        (StatusCode::SERVICE_UNAVAILABLE, "try again")
                    }} else {{ text(headers).await }}
                }}
            }}))
            .route("/text", get(text))
            .route("/events", get(|headers: HeaderMap| async move {{
                let (status, value) = text(headers).await;
                (status, [("content-type", "text/event-stream")], format!("data: {{value}}\n\n"))
            }}))
            .route("/large/text", get(|| async {{ "x".repeat(64) }}))
            .route("/collision", get(|| async {{ StatusCode::NO_CONTENT }}))
            .route("/config-collision", get(|| async {{ StatusCode::NO_CONTENT }}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{{}}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {{ axum::serve(listener, app).await.unwrap() }});
        let mut headers = HeaderMap::new();
        headers.insert("x-transport", "custom".parse().unwrap());
        let transport = reqwest::Client::builder().default_headers(headers).https_only(false).no_proxy().build().unwrap();
        {checks}
        server.abort();
    }}
}}
"#)).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/generated-custom-http-client"),
        )
        .env(
            "CARGO_BUILD_BUILD_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/generated-custom-http-client/build"),
        )
        .env_remove("CC_aarch64_apple_darwin")
        .env_remove("CXX_aarch64_apple_darwin")
        .env("CC", "cc")
        .env("CXX", "c++")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated client failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
