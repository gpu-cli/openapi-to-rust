use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::server::codegen::ServerCodegen;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn response_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "response semantics", "version": "1.0.0" },
        "paths": {
            "/things": { "post": {
                "operationId": "createThing",
                "tags": ["Things"],
                "responses": {
                    "201": { "$ref": "#/components/responses/CreatedAlias" },
                    "202": { "description": "accepted" },
                    "204": { "description": "no content" },
                    "2XX": { "description": "other success", "content": {
                        "application/json": { "schema": { "$ref": "#/components/schemas/Thing" } }
                    }},
                    "default": { "description": "other response", "content": {
                        "application/problem+json": { "schema": { "$ref": "#/components/schemas/Problem" } }
                    }}
                }
            }},
            "/stream": { "get": {
                "operationId": "streamThing",
                "tags": ["Things"],
                "responses": {
                    "202": { "description": "stream accepted", "content": {
                        "Text/Event-Stream; charset=utf-8": {}
                    }}
                }
            }},
            "/not-stream": { "get": {
                "operationId": "notStream",
                "responses": {
                    "200": { "description": "not SSE", "content": {
                        "text/event-streaming": {}
                    }}
                }
            }},
            "/text": { "get": {
                "operationId": "getText",
                "tags": ["Things"],
                "responses": {
                    "200": { "description": "text", "content": {
                        "text/plain; charset=utf-8": { "schema": { "type": "string" } }
                    }}
                }
            }},
            "/binary": { "get": {
                "operationId": "getBinary",
                "tags": ["Things"],
                "responses": {
                    "200": { "description": "binary", "content": {
                        "image/png": { "schema": { "type": "string", "format": "binary" } }
                    }}
                }
            }},
            "/wildcard": { "get": {
                "operationId": "getWildcard",
                "tags": ["Things"],
                "responses": {
                    "2XX": { "description": "binary", "content": {
                        "*/*": { "schema": { "type": "string", "format": "binary" } }
                    }}
                }
            }}
        },
        "components": {
            "schemas": {
                "Thing": { "type": "string" },
                "Problem": { "type": "string" }
            },
            "responses": {
                "CreatedAlias": { "$ref": "#/components/responses/Created" },
                "Created": { "description": "created", "content": {
                    "application/vnd.example+json": {
                        "schema": { "$ref": "#/components/schemas/Thing" }
                    }
                }}
            }
        }
    })
}

#[test]
fn analysis_retains_resolved_bodyless_media_and_exact_sse_responses() {
    let analysis = SchemaAnalyzer::new(response_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let create = &analysis.operation_responses["createThing"];
    assert_eq!(create["201"].schema_name.as_deref(), Some("Thing"));
    assert_eq!(
        create["201"].media_type.as_deref(),
        Some("application/vnd.example+json")
    );
    assert!(create["202"].schema_name.is_none());
    assert!(create["204"].schema_name.is_none());
    assert!(analysis.operation_responses["streamThing"]["202"].supports_streaming);
    assert!(!analysis.operation_responses["notStream"]["200"].supports_streaming);
    assert!(matches!(
        analysis.operation_responses["getText"]["200"].body,
        Some(openapi_to_rust::analysis::OperationResponseBody::Text { .. })
    ));
    assert!(matches!(
        analysis.operation_responses["getBinary"]["200"].body,
        Some(openapi_to_rust::analysis::OperationResponseBody::Binary {
            wildcard: false,
            ..
        })
    ));
    assert!(matches!(
        analysis.operation_responses["getWildcard"]["2XX"].body,
        Some(openapi_to_rust::analysis::OperationResponseBody::Binary { wildcard: true, .. })
    ));
}

#[test]
fn analysis_resolves_structurally_valid_response_from_any_local_pointer() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "misplaced response", "version": "1.0.0" },
        "paths": { "/things": { "get": {
            "operationId": "getThing",
            "responses": {
                "200": { "$ref": "#/components/responses/MisplacedAlias" }
            }
        }}},
        "components": {
            "schemas": {
                "Thing": { "type": "string" }
            },
            "responses": {
                "MisplacedAlias": {
                    "$ref": "#/components/requestBodies/MisplacedResponse"
                }
            },
            "requestBodies": {
                "MisplacedResponse": {
                    "description": "stored under the wrong component map",
                    "content": {
                        "application/vnd.example+json": {
                            "schema": { "$ref": "#/components/schemas/Thing" }
                        }
                    }
                }
            }
        }
    });

    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let response = &analysis.operation_responses["getThing"]["200"];
    assert_eq!(response.schema_name.as_deref(), Some("Thing"));
    assert_eq!(
        response.media_type.as_deref(),
        Some("application/vnd.example+json")
    );
}

#[test]
fn invalid_response_references_fail_with_actionable_errors() {
    let cases = [
        (
            "missing",
            "#/components/responses/Missing",
            json!({}),
            "does not exist",
        ),
        (
            "external",
            "https://example.invalid/responses.json#/Success",
            json!({}),
            "external response reference",
        ),
        (
            "incompatible",
            "#/components/schemas/NotAResponse",
            json!({
                "schemas": {
                    "NotAResponse": { "type": "string" }
                }
            }),
            "structurally compatible OpenAPI Response Object",
        ),
    ];

    for (name, reference, components, expected) in cases {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": name, "version": "1.0.0" },
            "paths": { "/things": { "get": {
                "operationId": "getThing",
                "responses": { "200": { "$ref": reference } }
            }}},
            "components": components
        });
        let error = SchemaAnalyzer::new(spec)
            .unwrap()
            .analyze()
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{name}: {error}");
    }
}

#[test]
fn cyclic_response_reference_chain_is_rejected() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "cyclic responses", "version": "1.0.0" },
        "paths": { "/things": { "get": {
            "operationId": "getThing",
            "responses": {
                "200": { "$ref": "#/components/responses/A" }
            }
        }}},
        "components": { "responses": {
            "A": { "$ref": "#/components/responses/B" },
            "B": { "$ref": "#/components/responses/A" }
        }}
    });

    let error = SchemaAnalyzer::new(spec)
        .unwrap()
        .analyze()
        .unwrap_err()
        .to_string();
    assert!(error.contains("Circular dependency"), "{error}");
    assert!(error.contains("response reference"), "{error}");
}

#[test]
fn generated_responses_preserve_status_ranges_media_and_sse_status() {
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let mut analysis = SchemaAnalyzer::new(response_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec![
            "createThing".into(),
            "streamThing".into(),
            "getText".into(),
            "getBinary".into(),
            "getWildcard".into(),
        ],
        prune_models: true,
        validation: ServerValidationSection {
            enabled: false,
            ..Default::default()
        },
    };
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "responses".into(),
        enable_async_client: false,
        server: Some(server),
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
    use axum::{http::{header::CONTENT_TYPE, StatusCode}, response::IntoResponse};

    #[test]
    fn response_contract_is_preserved() {
        let created = CreateThingResponse::Created("created".to_string()).into_response();
        assert_eq!(created.status(), StatusCode::CREATED);
        assert_eq!(created.headers()[CONTENT_TYPE], "application/vnd.example+json");

        assert_eq!(
            CreateThingResponse::Accepted.into_response().status(),
            StatusCode::ACCEPTED,
        );
        assert_eq!(
            CreateThingResponse::NoContent.into_response().status(),
            StatusCode::NO_CONTENT,
        );
        assert_eq!(
            CreateThingResponse::Success(StatusCode::MULTI_STATUS, "multi".to_string())
                .into_response()
                .status(),
            StatusCode::MULTI_STATUS,
        );
        assert_eq!(
            CreateThingResponse::Success(StatusCode::BAD_REQUEST, "wrong".to_string())
                .into_response()
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR,
        );
        let default = CreateThingResponse::Default(
            StatusCode::IM_A_TEAPOT,
            "problem".to_string(),
        ).into_response();
        assert_eq!(default.status(), StatusCode::IM_A_TEAPOT);
        assert_eq!(default.headers()[CONTENT_TYPE], "application/problem+json");

        let text = GetTextResponse::Ok("hello".to_string()).into_response();
        assert_eq!(text.status(), StatusCode::OK);
        assert_eq!(text.headers()[CONTENT_TYPE], "text/plain; charset=utf-8");

        let binary = GetBinaryResponse::Ok(bytes::Bytes::from_static(&[0, 159, 146, 150]))
            .into_response();
        assert_eq!(binary.status(), StatusCode::OK);
        assert_eq!(binary.headers()[CONTENT_TYPE], "image/png");

        let wildcard = GetWildcardResponse::Success(
            StatusCode::MULTI_STATUS,
            axum::http::HeaderValue::from_static("image/webp"),
            bytes::Bytes::from_static(&[0, 255]),
        ).into_response();
        assert_eq!(wildcard.status(), StatusCode::MULTI_STATUS);
        assert_eq!(wildcard.headers()[CONTENT_TYPE], "image/webp");
        assert_eq!(
            GetWildcardResponse::Success(
                StatusCode::BAD_REQUEST,
                axum::http::HeaderValue::from_static("image/webp"),
                bytes::Bytes::new(),
            ).into_response().status(),
            StatusCode::INTERNAL_SERVER_ERROR,
        );

        let stream = futures_util::stream::empty::<
            Result<axum::response::sse::Event, std::convert::Infallible>
        >();
        let streamed = StreamThingResponse::AcceptedStream(sse_response(stream)).into_response();
        assert_eq!(streamed.status(), StatusCode::ACCEPTED);
        assert_eq!(streamed.headers()[CONTENT_TYPE], "text/event-stream");
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "server-response-semantics"
version = "0.1.0"
edition = "2024"

[dependencies]
async-trait = "0.1"
axum = "0.8"
bytes = "1"
futures-core = "0.3"
futures-util = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
url = "2"
serde_urlencoded = "0.7"
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .arg("--offline")
        .current_dir(temp.path())
        .env(
            "CARGO_BUILD_BUILD_DIR",
            "/private/tmp/non-json-media-target/server-runtime-build",
        )
        .env(
            "CARGO_TARGET_DIR",
            "/private/tmp/non-json-media-target/server-runtime-target",
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated response server failed its runtime test:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn text_response_content_generates_a_string_variant() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "unsupported response", "version": "1.0.0" },
        "paths": { "/report": { "get": {
            "operationId": "getReport",
            "responses": { "200": { "description": "report", "content": {
                "text/plain": { "schema": { "type": "string" } }
            }}}
        }}}
    });
    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["getReport".into()],
        prune_models: false,
        validation: Default::default(),
    };
    let config = GeneratorConfig {
        enable_async_client: false,
        server: Some(server.clone()),
        ..Default::default()
    };
    let files = ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .expect("text/plain is a supported response body");
    let errors = files
        .iter()
        .find(|file| file.path.ends_with("errors.rs"))
        .expect("generated errors.rs");
    assert!(errors.content.contains("Ok(String)"), "{}", errors.content);
    assert!(errors.content.contains("text/plain"), "{}", errors.content);
}

#[test]
fn text_wildcard_response_is_rejected_instead_of_emitting_wildcard_content_type() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "wildcard text response", "version": "1.0.0" },
        "paths": { "/report": { "get": {
            "operationId": "getReport",
            "responses": { "200": { "description": "report", "content": {
                "text/*": { "schema": { "type": "string" } }
            }}}
        }}}
    });
    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let response = &analysis.operation_responses["getReport"]["200"];
    assert!(response.body.is_none());
    assert_eq!(response.unsupported_media_types, ["text/*"]);

    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["getReport".into()],
        prune_models: false,
        validation: Default::default(),
    };
    let config = GeneratorConfig {
        enable_async_client: false,
        server: Some(server.clone()),
        ..Default::default()
    };
    let error = ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .expect_err("text wildcard must not become a literal Content-Type")
        .to_string();
    assert!(error.contains("text/*"), "{error}");
}

#[test]
fn supported_json_representation_allows_unsupported_alternatives() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "mixed response", "version": "1.0.0" },
        "paths": { "/report": { "get": {
            "operationId": "getReport",
            "responses": { "200": { "description": "report", "content": {
                "application/json": { "schema": { "type": "string" } },
                "application/xml": { "schema": { "type": "string" } }
            }}}
        }}}
    });
    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let response = &analysis.operation_responses["getReport"]["200"];
    assert!(matches!(
        response.body,
        Some(openapi_to_rust::analysis::OperationResponseBody::Json { .. })
    ));
    assert_eq!(response.unsupported_media_types, ["application/xml"]);
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["getReport".into()],
        prune_models: false,
        validation: Default::default(),
    };
    let config = GeneratorConfig {
        enable_async_client: false,
        server: Some(server.clone()),
        ..Default::default()
    };
    ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .expect("the supported JSON representation should be generated");
}

#[test]
fn schema_bearing_vendor_json_wins_over_schema_less_canonical_json() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "JSON fallback", "version": "1.0.0" },
        "paths": { "/report": { "get": {
            "operationId": "getReport",
            "responses": { "200": { "description": "report", "content": {
                "application/json": {},
                "application/vnd.example+json": { "schema": { "type": "string" } }
            }} }
        }} }
    });
    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let response = &analysis.operation_responses["getReport"]["200"];
    assert!(matches!(
        &response.body,
        Some(openapi_to_rust::analysis::OperationResponseBody::Json { media_type, .. })
            if media_type == "application/vnd.example+json"
    ));
    assert_eq!(response.unsupported_media_types, ["application/json"]);
}
