use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::server::codegen::ServerCodegen;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

#[test]
fn generated_parameters_and_form_are_typed_validated_and_redacted() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "parameter media validation", "version": "1" },
        "paths": {
            "/items/{item_id}": { "get": {
                "operationId": "checkItem",
                "parameters": [
                    { "name": "item_id", "in": "path", "required": true,
                      "schema": { "type": "integer", "minimum": 1 } },
                    { "name": "limit", "in": "query", "required": true,
                      "schema": { "type": "integer", "maximum": 10 } },
                    { "name": "mode", "in": "query",
                      "schema": { "type": "string", "enum": ["safe", "fast"] } },
                    { "name": "x-level", "in": "header", "required": true,
                      "schema": { "type": "integer", "minimum": 2 } },
                    { "name": "1-mode", "in": "header",
                      "schema": { "type": "string" } },
                    { "name": "session", "in": "cookie", "required": true,
                      "schema": { "type": "string", "maxLength": 4 } }
                ],
                "responses": { "204": { "description": "ok" } }
            }},
            "/form": { "post": {
                "operationId": "submitForm",
                "requestBody": { "required": true, "content": {
                    "application/x-www-form-urlencoded": {
                        "schema": { "$ref": "#/components/schemas/FormPayload" }
                    }
                }},
                "responses": { "204": { "description": "ok" } }
            }}
        },
        "components": { "schemas": {
            "FormPayload": {
                "type": "object", "additionalProperties": false,
                "required": ["name", "count"],
                "properties": {
                    "name": { "type": "string", "minLength": 3 },
                    "count": { "type": "integer", "minimum": 1, "default": 2 }
                }
            }
        }}
    });
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let config = GeneratorConfig {
        output_dir,
        enable_async_client: false,
        tracing_enabled: false,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["checkItem".into(), "submitForm".into()],
            prune_models: true,
            validation: ServerValidationSection {
                enabled: true,
                max_body_bytes: 96,
                max_errors: 8,
            },
        }),
        ..Default::default()
    };
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    assert_eq!(
        analysis.operations["checkItem"]
            .parameters
            .iter()
            .find(|parameter| parameter.name == "session")
            .unwrap()
            .validation_schema
            .as_ref()
            .and_then(|schema| schema.get("maxLength")),
        Some(&json!(4))
    );
    let generator = CodeGenerator::new(config);
    let generated = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&generated).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "generated-parameter-media-validation"
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
serde_urlencoded = "0.7"
tokio = { version = "1", features = ["macros", "rt"] }
tower = { version = "0.5", features = ["util"] }
url = "2"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r####"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::{FormPayload, server::*};
    use axum::{body::{Body, to_bytes}, http::{Request, StatusCode, header::CONTENT_TYPE}};
    use serde_json::Value;
    use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
    use tower::ServiceExt;

    #[derive(Clone)]
    struct Api(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl ServerApi for Api {
        async fn check_item(&self, item_id: i64, limit: i64, mode: Option<CheckItemMode>, x_level: i64, _1_mode: Option<String>, session: String) -> CheckItemResponse {
            assert_eq!((item_id, limit, mode, x_level, _1_mode, session.as_str()), (2, 3, None, 4, None, "safe"));
            self.0.fetch_add(1, Ordering::SeqCst);
            CheckItemResponse::NoContent
        }

        async fn submit_form(&self, body: FormPayload) -> SubmitFormResponse {
            assert_eq!(body.name, "valid");
            assert_eq!(body.count, 2);
            self.0.fetch_add(1, Ordering::SeqCst);
            SubmitFormResponse::NoContent
        }
    }

    fn get(uri: &str, level: Option<&str>, cookie: Option<&str>) -> Request<Body> {
        let mut request = Request::builder().uri(uri);
        if let Some(level) = level { request = request.header("x-level", level); }
        if let Some(cookie) = cookie { request = request.header("cookie", cookie); }
        request.body(Body::empty()).unwrap()
    }

    fn form(body: &str) -> Request<Body> {
        Request::builder().method("POST").uri("/form")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string())).unwrap()
    }

    async fn problem(response: axum::response::Response, status: StatusCode) -> Value {
        assert_eq!(response.status(), status);
        assert_eq!(response.headers()[CONTENT_TYPE], "application/problem+json");
        let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[tokio::test]
    async fn transport_and_schema_failures_never_reach_the_handler_or_echo_values() {
        let calls = Arc::new(AtomicUsize::new(0));
        let app = server_api_router(Api(calls.clone()));

        assert_eq!(app.clone().oneshot(get("/items/2?limit=3", Some("4"), Some("session=safe"))).await.unwrap().status(), StatusCode::NO_CONTENT);
        assert_eq!(app.clone().oneshot(form("name=valid&count=2")).await.unwrap().status(), StatusCode::NO_CONTENT);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        problem(app.clone().oneshot(get("/items/not-a-number?limit=3", Some("4"), Some("session=safe"))).await.unwrap(), StatusCode::BAD_REQUEST).await;
        problem(app.clone().oneshot(get("/items/2?limit=%ZZ", Some("4"), Some("session=safe"))).await.unwrap(), StatusCode::BAD_REQUEST).await;
        problem(app.clone().oneshot(get("/items/2?limit=3&mode=private", Some("4"), Some("session=safe"))).await.unwrap(), StatusCode::UNPROCESSABLE_ENTITY).await;
        problem(app.clone().oneshot(get("/items/2?limit=3", None, Some("session=safe"))).await.unwrap(), StatusCode::UNPROCESSABLE_ENTITY).await;
        let secret = problem(app.clone().oneshot(get("/items/2?limit=3", Some("4"), Some("session=TOP_SECRET"))).await.unwrap(), StatusCode::UNPROCESSABLE_ENTITY).await;
        assert!(!serde_json::to_string(&secret).unwrap().contains("TOP_SECRET"));
        let invalid_form = problem(app.clone().oneshot(form("name=TOP_SECRET&count=0")).await.unwrap(), StatusCode::UNPROCESSABLE_ENTITY).await;
        assert!(!serde_json::to_string(&invalid_form).unwrap().contains("TOP_SECRET"));
        problem(app.clone().oneshot(form("name=%ZZ&count=2")).await.unwrap(), StatusCode::BAD_REQUEST).await;
        problem(app.clone().oneshot(form("name=valid&count=2&private=SECRET")).await.unwrap(), StatusCode::UNPROCESSABLE_ENTITY).await;
        problem(app.clone().oneshot(form("name=valid")).await.unwrap(), StatusCode::UNPROCESSABLE_ENTITY).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
"####,
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", temp.path().join("target"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated parameter/media validation failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn simple_array_header_is_typed_for_client_and_server() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "array header", "version": "1" },
        "paths": { "/attributes": { "get": {
            "operationId": "getAttributes",
            "parameters": [{
                "name": "x-object-attributes",
                "in": "header",
                "required": true,
                "schema": {
                    "type": "array",
                    "items": { "$ref": "#/components/schemas/ObjectAttribute" }
                }
            }],
            "responses": { "204": { "description": "ok" } }
        }}},
        "components": { "schemas": {
            "ObjectAttribute": {
                "type": "string",
                "enum": ["ETag", "ObjectSize"]
            }
        }}
    });
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["getAttributes".into()],
        prune_models: true,
        validation: Default::default(),
    };
    let config = GeneratorConfig {
        server: Some(server.clone()),
        ..Default::default()
    };
    let files = ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .expect("simple array headers should generate");
    let joined = files
        .into_iter()
        .map(|file| file.content)
        .collect::<String>();
    assert!(joined.contains("Vec<ObjectAttribute>"), "{joined}");
    assert!(joined.contains("split(',')"), "{joined}");

    let generator = CodeGenerator::new(config);
    let generated = generator.generate_all(&mut analysis).unwrap();
    let client = generated
        .files
        .iter()
        .find(|file| file.path.ends_with("client.rs"))
        .expect("generated client");
    assert!(
        client
            .content
            .contains("x_object_attributes: Vec<ObjectAttribute>"),
        "{}",
        client.content
    );
}

#[test]
fn raw_parameter_schema_preserves_large_integer_constraints() {
    let maximum = 9_007_199_254_740_993_u64;
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "precise parameter", "version": "1" },
        "paths": { "/precise": { "get": {
            "operationId": "precise",
            "parameters": [{
                "name": "value", "in": "query",
                "schema": { "type": "integer", "maximum": maximum }
            }],
            "responses": { "204": { "description": "ok" } }
        }}}
    });
    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    assert_eq!(
        analysis.operations["precise"].parameters[0]
            .validation_schema
            .as_ref()
            .and_then(|schema| schema.get("maximum")),
        Some(&json!(maximum))
    );
}

#[test]
fn unconstrained_parameter_still_has_a_compiled_validation_target() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "unconstrained parameter", "version": "1" },
        "paths": { "/anything": { "get": {
            "operationId": "anything",
            "parameters": [{ "name": "value", "in": "query", "schema": {} }],
            "responses": { "204": { "description": "ok" } }
        }}}
    });
    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["anything".into()],
        prune_models: true,
        validation: Default::default(),
    };
    let config = GeneratorConfig {
        server: Some(server.clone()),
        ..Default::default()
    };
    ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .expect("an empty schema is a real always-valid target");
}
