//! Live generated-client and independent-client compatibility for request validation.
//!
//! This deliberately crosses a real loopback TCP connection. Token snapshots and
//! in-process `Router::oneshot` tests cannot prove that the generated client,
//! generated Axum server, status taxonomy, headers, and public JSON contract all
//! agree on the wire.

use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn roundtrip_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "validation round trip", "version": "1.0.0" },
        "paths": {
            "/widgets/{tenant}": {
                "post": {
                    "operationId": "createWidget",
                    "parameters": [
                        {
                            "name": "tenant",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string", "pattern": "^[a-z]+$" }
                        },
                        {
                            "name": "limit",
                            "in": "query",
                            "required": true,
                            "schema": { "type": "integer", "minimum": 1, "maximum": 10 }
                        },
                        {
                            "name": "x-mode",
                            "in": "header",
                            "required": true,
                            "schema": { "type": "string", "minLength": 2, "maxLength": 8 }
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/WidgetInput" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "created",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Widget" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "WidgetInput": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "secret"],
                    "properties": {
                        "name": { "type": "string", "minLength": 3, "maxLength": 12 },
                        "secret": { "type": "string", "maxLength": 4 }
                    }
                },
                "Widget": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["tenant", "name"],
                    "properties": {
                        "tenant": { "type": "string" },
                        "name": { "type": "string" }
                    }
                }
            }
        }
    })
}

#[test]
fn generated_and_handwritten_clients_round_trip_validation_problems_over_http() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::TempDir::new().expect("scratch crate");
    let output_dir = temp.path().join("src/generated");
    let config = GeneratorConfig {
        output_dir,
        module_name: "validation_roundtrip".into(),
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["createWidget".into()],
            prune_models: true,
            validation: ServerValidationSection {
                enabled: true,
                max_body_bytes: 128,
                max_errors: 4,
            },
        }),
        ..Default::default()
    };
    let mut analysis = SchemaAnalyzer::new(roundtrip_spec())
        .expect("analyzer")
        .analyze()
        .expect("analysis");
    let generator = CodeGenerator::new(config);
    let generated = generator
        .generate_all(&mut analysis)
        .expect("client and server generation");
    generator.write_files(&generated).expect("generated files");

    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "generated-validation-roundtrip"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
async-trait = "0.1"
axum = { version = "0.8", default-features = false, features = ["http1", "json", "tokio"] }
http-body-util = "0.1"
jsonschema = { version = "0.49", default-features = false }
mime = "0.3"
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
reqwest-middleware = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_urlencoded = "0.7"
thiserror = "1"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "sync", "time"] }
url = "2"
"#,
    )
    .expect("scratch manifest");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r####"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::*;
    use reqwest::header::CONTENT_TYPE;
    use serde::Deserialize;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::oneshot;

    const SECRET: &str = "TOP_SECRET_MARKER";

    #[derive(Clone)]
    struct Api {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ServerApi for Api {
        async fn create_widget(
            &self,
            tenant: String,
            limit: i64,
            x_mode: String,
            body: WidgetInput,
        ) -> CreateWidgetResponse {
            assert_eq!(limit, 2);
            assert_eq!(x_mode, "sync");
            self.calls.fetch_add(1, Ordering::SeqCst);
            CreateWidgetResponse::Ok(Widget {
                tenant,
                name: body.name,
            })
        }
    }

    #[derive(Debug, Deserialize)]
    struct HandProblem {
        #[serde(rename = "type")]
        problem_type: String,
        title: String,
        status: u16,
        code: String,
        #[serde(default)]
        errors: Vec<HandViolation>,
    }

    #[derive(Debug, Deserialize)]
    struct HandViolation {
        code: String,
        location: String,
        message: String,
    }

    async fn hand_problem(
        response: reqwest::Response,
        expected_status: reqwest::StatusCode,
        expected_code: &str,
    ) -> (HandProblem, String) {
        assert_eq!(response.status(), expected_status);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        let body = response.text().await.unwrap();
        for forbidden in [SECRET, "jsonschema", "serde_json", "ValidationError", "backtrace"] {
            assert!(!body.contains(forbidden), "public body leaked `{forbidden}`: {body}");
        }
        let problem: HandProblem = serde_json::from_str(&body).unwrap();
        assert_eq!(problem.status, expected_status.as_u16());
        assert_eq!(problem.code, expected_code);
        assert!(problem.problem_type.starts_with("https://openapi-to-rust.dev/problems/"));
        assert!(!problem.title.is_empty());
        (problem, body)
    }

    #[tokio::test]
    async fn live_wire_contract_is_typed_sanitized_and_handler_isolated() {
        let calls = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let server_calls = calls.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, server_api_router(Api { calls: server_calls }))
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });
        let base_url = format!("http://{address}");
        let generated_client = HttpClient::new().with_base_url(base_url.clone());

        let created = generated_client
            .create_widget(
                "acme",
                2,
                "sync",
                WidgetInput {
                    name: "useful".into(),
                    secret: "safe".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(created.tenant, "acme");
        assert_eq!(created.name, "useful");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let generated_error = generated_client
            .create_widget(
                "acme",
                2,
                "sync",
                WidgetInput {
                    name: "useful".into(),
                    secret: SECRET.into(),
                },
            )
            .await
            .unwrap_err();
        let api_error = generated_error.api().expect("HTTP response error");
        assert_eq!(api_error.status, 422);
        assert_eq!(api_error.headers[CONTENT_TYPE], "application/problem+json");
        assert!(!api_error.body.contains(SECRET));
        let typed_problem = api_error.problem_details().expect("typed problem details");
        assert_eq!(typed_problem.status, 422);
        assert_eq!(typed_problem.code, "request_validation_failed");
        assert_eq!(typed_problem.errors.len(), 1);
        assert_eq!(typed_problem.errors[0].code, "max_length");
        assert_eq!(typed_problem.errors[0].location, "/body/secret");
        assert_eq!(typed_problem.errors[0].message, "does not meet the length constraint");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let hand = reqwest::Client::new();
        let malformed = hand
            .post(format!("{base_url}/widgets/acme?limit=2"))
            .header("x-mode", "sync")
            .header(CONTENT_TYPE, "application/json")
            .body(format!(r#"{{"name":"{SECRET}""#))
            .send()
            .await
            .unwrap();
        hand_problem(
            malformed,
            reqwest::StatusCode::BAD_REQUEST,
            "malformed_request",
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let oversized = hand
            .post(format!("{base_url}/widgets/acme?limit=2"))
            .header("x-mode", "sync")
            .header(CONTENT_TYPE, "application/json")
            .body(format!(
                r#"{{"name":"valid","secret":"{}"}}"#,
                SECRET.repeat(16)
            ))
            .send()
            .await
            .unwrap();
        hand_problem(
            oversized,
            reqwest::StatusCode::PAYLOAD_TOO_LARGE,
            "request_body_too_large",
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let unsupported = hand
            .post(format!("{base_url}/widgets/acme?limit=2"))
            .header("x-mode", "sync")
            .body(format!(r#"{{"name":"valid","secret":"{SECRET}"}}"#))
            .send()
            .await
            .unwrap();
        hand_problem(
            unsupported,
            reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let invalid = hand
            .post(format!("{base_url}/widgets/acme?limit=2"))
            .header("x-mode", "sync")
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(format!(r#"{{"name":"x","secret":"{SECRET}"}}"#))
            .send()
            .await
            .unwrap();
        let (problem, _) = hand_problem(
            invalid,
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
            "request_validation_failed",
        )
        .await;
        assert_eq!(
            problem
                .errors
                .iter()
                .map(|error| (&*error.location, &*error.code, &*error.message))
                .collect::<Vec<_>>(),
            vec![
                ("/body/name", "min_length", "does not meet the length constraint"),
                ("/body/secret", "max_length", "does not meet the length constraint"),
            ]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        shutdown_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
}
"####,
    )
    .expect("scratch lib");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/server-validation-roundtrip-smoke"),
        )
        .output()
        .expect("scratch cargo test");
    assert!(
        output.status.success(),
        "generated validation round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
