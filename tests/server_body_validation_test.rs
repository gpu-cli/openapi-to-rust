use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::server::codegen::ServerCodegen;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn body_validation_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "body validation", "version": "1.0.0" },
        "paths": {
            "/payload": {
                "post": {
                    "operationId": "createPayload",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Payload" }
                            }
                        }
                    },
                    "responses": { "204": { "description": "accepted" } }
                }
            },
            "/optional": {
                "post": {
                    "operationId": "maybePayload",
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Payload" }
                            }
                        }
                    },
                    "responses": { "204": { "description": "accepted" } }
                }
            }
        },
        "components": {
            "schemas": {
                "Payload": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["name", "count"],
                    "properties": {
                        "name": { "type": "string", "minLength": 3, "pattern": "^[a-z]+$" },
                        "count": { "type": "integer", "minimum": 1, "maximum": 10 }
                    }
                }
            }
        }
    })
}

#[test]
fn programmatic_validation_limits_are_checked_at_generation_boundary() {
    for validation in [
        ServerValidationSection {
            enabled: true,
            max_body_bytes: 96,
            max_errors: 0,
        },
        ServerValidationSection {
            enabled: true,
            max_body_bytes: 0,
            max_errors: 2,
        },
    ] {
        let config = GeneratorConfig {
            enable_async_client: false,
            tracing_enabled: false,
            server: Some(ServerSection {
                framework: "axum".into(),
                operations: vec!["createPayload".into()],
                prune_models: true,
                validation,
            }),
            ..Default::default()
        };
        let mut analysis = SchemaAnalyzer::new(body_validation_spec())
            .unwrap()
            .analyze()
            .unwrap();
        let error = CodeGenerator::new(config)
            .generate_all(&mut analysis)
            .unwrap_err()
            .to_string();
        assert!(error.contains("must be between"), "{error}");
    }
}

#[test]
fn unsupported_request_media_is_rejected_during_server_generation() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "unsupported body", "version": "1" },
        "paths": { "/xml": { "post": {
            "operationId": "postXml",
            "requestBody": { "required": true, "content": {
                "application/xml": { "schema": { "type": "string" } }
            }},
            "responses": { "204": { "description": "unused" } }
        }}}
    });
    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let analysis = analyzer.analyze().unwrap();
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["postXml".into()],
        prune_models: true,
        validation: Default::default(),
    };
    let config = GeneratorConfig {
        server: Some(server.clone()),
        ..Default::default()
    };
    let error = ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .unwrap_err()
        .to_string();
    assert!(error.contains("postXml"), "{error}");
    assert!(error.contains("application/xml"), "{error}");
}

#[test]
fn generated_json_pipeline_validates_before_invoking_the_api() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::TempDir::new().expect("scratch crate");
    let output_dir = temp.path().join("src/generated");
    let config = GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "body_validation".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["createPayload".into(), "maybePayload".into()],
            prune_models: true,
            validation: ServerValidationSection {
                enabled: true,
                max_body_bytes: 96,
                max_errors: 2,
            },
        }),
        ..Default::default()
    };
    let mut analyzer = SchemaAnalyzer::new(body_validation_spec()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(config);
    let result = generator
        .generate_all(&mut analysis)
        .expect("generation succeeds");
    generator.write_files(&result).expect("generated files");

    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "generated-body-validation"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
async-trait = "0.1"
axum = { version = "0.8", default-features = false, features = ["json"] }
jsonschema = { version = "0.49", default-features = false }
mime = "0.3"
http-body-util = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
tower = { version = "0.5", features = ["util"] }
"#,
    )
    .expect("scratch manifest");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r####"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::Payload;
    use super::generated::server::*;
    use super::generated::server::validation::{
        self, VALIDATION_TARGET_0_BODY,
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header::CONTENT_TYPE},
        response::IntoResponse,
    };
    use serde_json::Value;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    use tower::ServiceExt;

    #[derive(Clone)]
    struct Api {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ServerApi for Api {
        async fn create_payload(&self, _body: Payload) -> CreatePayloadResponse {
            self.calls.fetch_add(1, Ordering::SeqCst);
            CreatePayloadResponse::Empty
        }

        async fn maybe_payload(&self, _body: Option<Payload>) -> MaybePayloadResponse {
            self.calls.fetch_add(1, Ordering::SeqCst);
            MaybePayloadResponse::Empty
        }
    }

    fn request(path: &str, content_type: Option<&str>, body: impl Into<Body>) -> Request<Body> {
        let mut builder = Request::builder().method("POST").uri(path);
        if let Some(content_type) = content_type {
            builder = builder.header(CONTENT_TYPE, content_type);
        }
        builder.body(body.into()).unwrap()
    }

    async fn assert_problem(
        response: axum::response::Response,
        status: StatusCode,
        code: &str,
    ) -> Value {
        assert_eq!(response.status(), status);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/problem+json"
        );
        let bytes = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let problem: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(problem["code"], code);
        problem
    }

    #[tokio::test]
    async fn status_taxonomy_and_handler_isolation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let app = server_api_router(Api { calls: calls.clone() });

        let valid = app.clone().oneshot(request(
            "/payload",
            Some("application/json; charset=utf-8"),
            r#"{"name":"valid","count":2}"#,
        )).await.unwrap();
        assert_eq!(valid.status(), StatusCode::NO_CONTENT);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let malformed = app.clone().oneshot(request(
            "/payload", Some("application/json"), r#"{"name":"secret""#,
        )).await.unwrap();
        let malformed_problem =
            assert_problem(malformed, StatusCode::BAD_REQUEST, "malformed_request").await;
        assert!(!serde_json::to_string(&malformed_problem).unwrap().contains("secret"));

        let missing_media = app.clone().oneshot(request(
            "/payload", None, r#"{"name":"valid","count":2}"#,
        )).await.unwrap();
        assert_problem(
            missing_media,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ).await;

        let wrong_media = app.clone().oneshot(request(
            "/payload", Some("text/plain"), r#"{"name":"valid","count":2}"#,
        )).await.unwrap();
        assert_problem(
            wrong_media,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ).await;

        let malformed_media = app.clone().oneshot(request(
            "/payload", Some("application/json; garbage"), r#"{"name":"valid","count":2}"#,
        )).await.unwrap();
        assert_problem(
            malformed_media,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ).await;

        let oversized_json = format!(r#"{{"name":"{}","count":2}}"#, "a".repeat(128));
        let oversized = app.clone().oneshot(request(
            "/payload", Some("application/json"), oversized_json,
        )).await.unwrap();
        assert_problem(
            oversized,
            StatusCode::PAYLOAD_TOO_LARGE,
            "request_body_too_large",
        ).await;

        let similar_but_undeclared_media = app.clone().oneshot(request(
            "/payload",
            Some("application/merge-patch+json"),
            r#"{"name":"TOP_SECRET","count":0,"private":"DO_NOT_ECHO"}"#,
        )).await.unwrap();
        assert_problem(
            similar_but_undeclared_media,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
        ).await;

        let invalid = app.clone().oneshot(request(
            "/payload",
            Some("application/json"),
            r#"{"name":"TOP_SECRET","count":0,"private":"DO_NOT_ECHO"}"#,
        )).await.unwrap();
        let problem = assert_problem(
            invalid,
            StatusCode::UNPROCESSABLE_ENTITY,
            "request_validation_failed",
        ).await;
        let encoded = serde_json::to_string(&problem).unwrap();
        assert!(!encoded.contains("TOP_SECRET"));
        assert!(!encoded.contains("DO_NOT_ECHO"));
        let errors = problem["errors"].as_array().unwrap();
        assert!(errors.len() <= 2);
        assert!(errors.windows(2).all(|pair| {
            pair[0]["location"].as_str() <= pair[1]["location"].as_str()
        }));

        let empty_required = app.clone().oneshot(request(
            "/payload", Some("application/json"), Body::empty(),
        )).await.unwrap();
        assert_problem(
            empty_required,
            StatusCode::BAD_REQUEST,
            "malformed_request",
        ).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let optional_absent = app.clone().oneshot(request(
            "/optional", None, Body::empty(),
        )).await.unwrap();
        assert_eq!(optional_absent.status(), StatusCode::NO_CONTENT);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let optional_explicit_empty = app.clone().oneshot(request(
            "/optional", Some("application/json"), Body::empty(),
        )).await.unwrap();
        assert_eq!(optional_explicit_empty.status(), StatusCode::NO_CONTENT);
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        let optional_invalid = app.clone().oneshot(request(
            "/optional", Some("application/json"), r#"{"name":"x","count":1}"#,
        )).await.unwrap();
        assert_problem(
            optional_invalid,
            StatusCode::UNPROCESSABLE_ENTITY,
            "request_validation_failed",
        ).await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        let optional_null = app.clone().oneshot(request(
            "/optional", Some("application/json"), "null",
        )).await.unwrap();
        assert_problem(
            optional_null,
            StatusCode::UNPROCESSABLE_ENTITY,
            "request_validation_failed",
        ).await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn typed_contract_mismatch_is_a_sanitized_500() {
        let rejection = validation::decode_json_body::<bool>(
            request(
                "/payload",
                Some("application/json"),
                r#"{"name":"valid","count":2}"#,
            ),
            VALIDATION_TARGET_0_BODY,
            "application/json",
            true,
            96,
        ).await.unwrap_err();
        let problem = assert_problem(
            rejection.into_response(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "generated_contract_error",
        ).await;
        let encoded = serde_json::to_string(&problem).unwrap();
        assert!(!encoded.contains("bool"));
        assert!(!encoded.contains("expected"));
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
            manifest_dir.join("target/generated-body-validation"),
        )
        .output()
        .expect("scratch cargo test");
    assert!(
        output.status.success(),
        "generated body validation crate failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
