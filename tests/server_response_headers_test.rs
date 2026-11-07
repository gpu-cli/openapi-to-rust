//! Declared response headers (`responses.*.headers`) in generated servers.

use openapi_to_rust::analysis::OperationResponseHeader;
use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn headers_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "response headers", "version": "1.0.0" },
        "paths": {
            "/login": { "post": {
                "operationId": "login",
                "tags": ["Auth"],
                "responses": {
                    "200": {
                        "description": "signed in",
                        "headers": {
                            "Set-Cookie": { "$ref": "#/components/headers/SetCookie" },
                            "X-Request-Id": {
                                "description": "Correlates logs for this request.",
                                "required": true,
                                "schema": { "type": "string" }
                            },
                            "Content-Type": { "schema": { "type": "string" } }
                        },
                        "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/Account" }
                        }}
                    },
                    "204": {
                        "description": "signed out",
                        "headers": {
                            "Link": {
                                "schema": { "type": "array", "items": { "type": "string" } }
                            }
                        }
                    },
                    "4XX": {
                        "description": "rejected",
                        "headers": {
                            "Retry-After": { "schema": { "type": "integer" } }
                        },
                        "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/Account" }
                        }}
                    }
                }
            }},
            "/plain": { "get": {
                "operationId": "plain",
                "tags": ["Auth"],
                "responses": {
                    "200": { "description": "no headers", "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/Account" }
                    }}}
                }
            }}
        },
        "components": {
            "schemas": {
                "Account": { "type": "object", "properties": { "id": { "type": "string" } } }
            },
            "headers": {
                "SetCookie": {
                    "description": "Session cookie.",
                    "schema": { "type": "string" }
                }
            }
        }
    })
}

#[test]
fn analysis_collects_declared_headers_except_content_type() {
    let analysis = SchemaAnalyzer::new(headers_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let login = &analysis.operation_responses["login"];
    assert_eq!(
        login["200"].headers,
        vec![
            // `$ref` resolved; Set-Cookie is always one line per value.
            OperationResponseHeader {
                name: "Set-Cookie".into(),
                description: Some("Session cookie.".into()),
                required: false,
                multiple: true,
            },
            OperationResponseHeader {
                name: "X-Request-Id".into(),
                description: Some("Correlates logs for this request.".into()),
                required: true,
                multiple: false,
            },
        ]
    );
    assert!(
        login["204"].headers[0].multiple,
        "array schemas are multi-valued"
    );
    assert!(
        analysis.operation_responses["plain"]["200"]
            .headers
            .is_empty()
    );
}

#[test]
fn invalid_header_names_are_rejected() {
    let mut spec = headers_spec();
    spec["paths"]["/plain"]["get"]["responses"]["200"]["headers"] =
        json!({ "Bad Header": { "schema": { "type": "string" } } });
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: temp.path().join("generated"),
        module_name: "headers".into(),
        enable_async_client: false,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["plain".into()],
            prune_models: true,
            validation: ServerValidationSection::default(),
        }),
        ..Default::default()
    });
    let error = generator
        .generate_all(&mut analysis)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Bad Header"), "{error}");
}

#[test]
fn generated_variants_carry_and_emit_declared_headers() {
    assert_generated_headers(headers_spec(), "Account", "LoginResponseOkHeaders");
}

#[test]
fn response_headers_avoid_model_names_and_roundtrip() {
    // Both the preferred name and the first fallback belong to retained models.
    let mut spec: serde_json::Value = serde_json::from_str(
        &headers_spec()
            .to_string()
            .replace("Account", "LoginResponseOkHeaders"),
    )
    .unwrap();
    spec["components"]["schemas"]["LoginResponseOkHeaders"]["properties"]["nested"] =
        json!({ "$ref": "#/components/schemas/LoginResponseOkHeaders2" });
    spec["components"]["schemas"]["LoginResponseOkHeaders2"] =
        json!({ "type": "object", "properties": { "value": { "type": "string" } } });
    assert_generated_headers(spec, "LoginResponseOkHeaders", "LoginResponseOkHeaders3");
}

#[test]
fn response_header_names_avoid_parameter_enums_and_are_stable_across_selectors() {
    let mut spec = headers_spec();
    spec["paths"]["/login"]["post"]["parameters"] = json!([{
        "name": "responseOkHeaders", "in": "query",
        "schema": { "type": "string", "enum": ["a", "b"] }
    }]);
    let temp = tempfile::TempDir::new().unwrap();
    let mut outputs = Vec::new();
    for operations in [["login", "plain"], ["plain", "login"]] {
        let mut analysis = SchemaAnalyzer::new(spec.clone())
            .unwrap()
            .analyze()
            .unwrap();
        let generator = CodeGenerator::new(GeneratorConfig {
            output_dir: temp.path().join("generated"),
            module_name: "headers".into(),
            enable_async_client: false,
            server: Some(ServerSection {
                framework: "axum".into(),
                operations: operations.map(String::from).to_vec(),
                prune_models: true,
                validation: ServerValidationSection::default(),
            }),
            ..Default::default()
        });
        let result = generator.generate_all(&mut analysis).unwrap();
        let errors = &result
            .files
            .iter()
            .find(|file| file.path == std::path::Path::new("server/errors.rs"))
            .unwrap()
            .content;
        assert!(
            errors.contains("pub struct LoginResponseOkHeaders2"),
            "{errors}"
        );
        // Declaration order follows selectors; public names and fields must not.
        let declarations: std::collections::BTreeMap<_, _> = syn::parse_file(errors)
            .unwrap()
            .items
            .into_iter()
            .filter_map(|item| {
                let name = match &item {
                    syn::Item::Struct(item) => item.ident.to_string(),
                    syn::Item::Enum(item) => item.ident.to_string(),
                    _ => return None,
                };
                Some((name, quote::quote!(#item).to_string()))
            })
            .collect();
        outputs.push(declarations);
    }
    assert_eq!(outputs[0], outputs[1]);
}

fn assert_generated_headers(spec: serde_json::Value, model: &str, ok_headers: &str) {
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "headers".into(),
        enable_async_client: false,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["login".into(), "plain".into()],
            prune_models: true,
            validation: ServerValidationSection {
                enabled: false,
                ..Default::default()
            },
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::server::*;
    use super::generated::types::__BODY_MODEL__;
    use axum::{http::{header, StatusCode}, response::IntoResponse};

    fn values(response: &axum::response::Response, name: header::HeaderName) -> Vec<String> {
        response
            .headers()
            .get_all(name)
            .iter()
            .map(|value| value.to_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn declared_headers_are_emitted() {
        let response = LoginResponse::Ok(
            __BODY_MODEL__::default(),
            __OK_HEADERS__ {
                set_cookie: vec![
                    "session=abc; HttpOnly; Secure".to_owned(),
                    "theme=dark, light".to_owned(),
                ],
                x_request_id: "req-1".to_owned(),
            },
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        // One `Set-Cookie` line per value, never comma-joined.
        assert_eq!(
            values(&response, header::SET_COOKIE),
            ["session=abc; HttpOnly; Secure", "theme=dark, light"],
        );
        assert_eq!(response.headers()["x-request-id"], "req-1");

        let empty = LoginResponse::NoContent(LoginResponseNoContentHeaders {
            link: vec!["</a>; rel=next".to_owned()],
        })
        .into_response();
        assert_eq!(empty.status(), StatusCode::NO_CONTENT);
        assert_eq!(values(&empty, header::LINK), ["</a>; rel=next"]);

        let limited = LoginResponse::ClientError(
            StatusCode::TOO_MANY_REQUESTS,
            __BODY_MODEL__::default(),
            LoginResponseClientErrorHeaders { retry_after: Some("30".to_owned()) },
        )
        .into_response();
        assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(limited.headers()[header::RETRY_AFTER], "30");
        let unset = LoginResponse::ClientError(
            StatusCode::BAD_REQUEST,
            __BODY_MODEL__::default(),
            LoginResponseClientErrorHeaders::default(),
        )
        .into_response();
        assert!(unset.headers().get(header::RETRY_AFTER).is_none());

        // Responses without declared headers keep their previous shape.
        assert_eq!(
            PlainResponse::Ok(__BODY_MODEL__::default()).into_response().status(),
            StatusCode::OK,
        );
    }

    #[test]
    fn invalid_header_values_fail_closed() {
        let response = LoginResponse::Ok(
            __BODY_MODEL__::default(),
            __OK_HEADERS__ {
                set_cookie: Vec::new(),
                x_request_id: "line\nbreak".to_owned(),
            },
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[derive(Clone)]
    struct Api;

    #[async_trait::async_trait]
    impl AuthApi for Api {
        async fn login(&self) -> LoginResponse {
            LoginResponse::Ok(
                __BODY_MODEL__::default(),
                __OK_HEADERS__ {
                    set_cookie: vec!["session=abc; HttpOnly".into(), "theme=dark".into()],
                    x_request_id: "wire-1".into(),
                },
            )
        }

        async fn plain(&self) -> PlainResponse {
            PlainResponse::Ok(__BODY_MODEL__::default())
        }
    }

    #[tokio::test]
    async fn headers_and_model_body_reach_http_client() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, auth_api_router(Api)).await.unwrap();
        });
        let client = reqwest::Client::new();
        let response = client.post(format!("http://{address}/login")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(response.headers()["x-request-id"], "wire-1");
        let cookies: Vec<_> = response.headers().get_all(header::SET_COOKIE)
            .iter().map(|value| value.to_str().unwrap()).collect();
        assert_eq!(cookies, ["session=abc; HttpOnly", "theme=dark"]);
        let _: __BODY_MODEL__ = response.json().await.unwrap();
        let response = client.get(format!("http://{address}/plain")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(header::SET_COOKIE).is_none());
        let _: __BODY_MODEL__ = response.json().await.unwrap();
        server.abort();
    }
}
"#
        .replace("__BODY_MODEL__", model)
        .replace("__OK_HEADERS__", ok_headers),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "server-response-headers"
version = "0.1.0"
edition = "2024"

[dependencies]
async-trait = "0.1"
axum = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
url = "2"
serde_urlencoded = "0.7"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
reqwest = { version = "0.13", default-features = false, features = ["json"] }
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .current_dir(temp.path())
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .env("CARGO_TARGET_DIR", temp.path().join("cargo-target"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated server failed its header test:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
