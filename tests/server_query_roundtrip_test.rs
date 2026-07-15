//! End-to-end query serialization symmetry for generated clients and Axum servers.
//!
//! The important contract here is the wire, not just matching token snapshots:
//! a generated `HttpClient` sends requests to a real generated Axum router and
//! the trait implementation observes the original typed values.

use openapi_to_rust::config::ServerSection;
use openapi_to_rust::server::codegen::ServerCodegen;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::process::Command;

fn round_trip_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "query round trip", "version": "1.0.0" },
        "paths": {
            "/round-trip": {
                "get": {
                    "operationId": "queryRoundTrip",
                    "parameters": [
                        {
                            "name": "page",
                            "in": "query",
                            "required": true,
                            "schema": { "type": "integer", "format": "int64" }
                        },
                        {
                            "name": "active",
                            "in": "query",
                            "schema": { "type": "boolean" }
                        },
                        {
                            "name": "required_expanded",
                            "in": "query",
                            "required": true,
                            "style": "form",
                            "explode": true,
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "color": { "type": "string" },
                                    "min_count": { "type": "integer", "format": "int32" }
                                }
                            }
                        },
                        {
                            "name": "optional_expanded",
                            "in": "query",
                            "style": "form",
                            "explode": true,
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "term": { "type": "string" },
                                    "archived": { "type": "boolean" }
                                }
                            }
                        },
                        {
                            "name": "compact",
                            "in": "query",
                            "required": true,
                            "style": "form",
                            "explode": false,
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "kind": { "type": "string" },
                                    "count": { "type": "integer", "format": "int32" }
                                }
                            }
                        },
                        {
                            "name": "deep_filter",
                            "in": "query",
                            "style": "deepObject",
                            "explode": true,
                            "schema": {
                                "type": "object",
                                "properties": {
                                    "owner": { "type": "string" },
                                    "open": { "type": "boolean" }
                                }
                            }
                        },
                        {
                            "name": "ids",
                            "in": "query",
                            "required": true,
                            "style": "form",
                            "explode": true,
                            "schema": {
                                "type": "array",
                                "items": { "type": "integer", "format": "int64" }
                            }
                        },
                        {
                            "name": "scores",
                            "in": "query",
                            "style": "form",
                            "explode": false,
                            "schema": { "$ref": "#/components/schemas/ScoresAlias" }
                        }
                    ],
                    "responses": { "204": { "description": "captured" } }
                }
            }
        },
        "components": {
            "schemas": {
                "ScoresAlias": { "$ref": "#/components/schemas/Scores" },
                "Scores": {
                    "type": "array",
                    "items": { "type": "integer", "format": "int32" }
                }
            }
        }
    })
}

#[test]
fn generated_client_and_server_round_trip_typed_query_parameters() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::TempDir::new().expect("temp crate");
    let output_dir = temp.path().join("src/generated");
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["queryRoundTrip".into()],
        prune_models: true,
    };
    let config = GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "query_round_trip".into(),
        enable_async_client: true,
        tracing_enabled: false,
        server: Some(server.clone()),
        ..Default::default()
    };
    let mut analyzer = SchemaAnalyzer::new(round_trip_spec()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(config);
    let result = generator
        .generate_all(&mut analysis)
        .expect("client and types generate");
    generator.write_files(&result).expect("client files write");
    for file in ServerCodegen::new(generator.config(), &analysis, &server)
        .generate()
        .expect("server generates")
    {
        let path = output_dir.join(file.path);
        std::fs::create_dir_all(path.parent().expect("server file parent"))
            .expect("server directory");
        std::fs::write(path, file.content).expect("server file");
    }
    std::fs::write(
        output_dir.join("mod.rs"),
        "pub mod types;\npub use types::*;\npub mod client;\npub use client::*;\npub mod server;\npub use server::*;\n",
    )
    .expect("generated mod.rs");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "generated-query-round-trip"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.7"
reqwest = { version = "0.12", features = ["json", "multipart"] }
reqwest-middleware = { version = "0.4", features = ["multipart"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_urlencoded = "0.7"
thiserror = "2"
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "sync", "time"] }
url = "2"
"#,
    )
    .expect("scratch manifest");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::*;
    use serde_json::{Value, json};
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    #[derive(Clone)]
    struct Api {
        captured: UnboundedSender<Value>,
    }

    #[axum::async_trait]
    impl ServerApi for Api {
        async fn query_round_trip(
            &self,
            page: i64,
            active: Option<bool>,
            required_expanded: QueryRoundTripRequiredExpanded,
            optional_expanded: Option<QueryRoundTripOptionalExpanded>,
            compact: QueryRoundTripCompact,
            deep_filter: Option<QueryRoundTripDeepFilter>,
            ids: Vec<i64>,
            scores: Option<Vec<i32>>,
        ) -> QueryRoundTripResponse {
            self.captured
                .send(json!({
                    "page": page,
                    "active": active,
                    "required_expanded": required_expanded,
                    "optional_expanded": optional_expanded,
                    "compact": compact,
                    "deep_filter": deep_filter,
                    "ids": ids,
                    "scores": scores,
                }))
                .unwrap();
            QueryRoundTripResponse::Empty
        }
    }

    #[tokio::test]
    async fn query_values_survive_the_generated_http_wire() {
        let (captured_tx, mut captured_rx) = unbounded_channel();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                server_api_router(Api { captured: captured_tx }),
            )
            .await
            .unwrap();
        });
        let client = HttpClient::new().with_base_url(format!("http://{address}"));

        client
            .query_round_trip(
                7,
                Some(true),
                QueryRoundTripRequiredExpanded {
                    color: Some("red".into()),
                    min_count: Some(2),
                },
                Some(QueryRoundTripOptionalExpanded {
                    term: Some("rust sdk".into()),
                    archived: Some(false),
                }),
                QueryRoundTripCompact {
                    kind: Some("public".into()),
                    count: Some(3),
                },
                Some(QueryRoundTripDeepFilter {
                    owner: Some("alice/bob".into()),
                    open: Some(true),
                }),
                vec![11, 12],
                Some(vec![8, 9]),
            )
            .await
            .unwrap();
        let full = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            captured_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            full,
            json!({
                "page": 7,
                "active": true,
                "required_expanded": { "color": "red", "min_count": 2 },
                "optional_expanded": { "term": "rust sdk", "archived": false },
                "compact": { "kind": "public", "count": 3 },
                "deep_filter": { "owner": "alice/bob", "open": true },
                "ids": [11, 12],
                "scores": [8, 9],
            })
        );

        client
            .query_round_trip(
                0,
                None,
                QueryRoundTripRequiredExpanded::default(),
                Some(QueryRoundTripOptionalExpanded::default()),
                QueryRoundTripCompact::default(),
                Some(QueryRoundTripDeepFilter::default()),
                Vec::new(),
                Some(Vec::new()),
            )
            .await
            .unwrap();
        let empty = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            captured_rx.recv(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            empty,
            json!({
                "page": 0,
                "active": null,
                "required_expanded": {},
                "optional_expanded": {},
                "compact": {},
                "deep_filter": {},
                "ids": [],
                "scores": [],
            })
        );

        let missing_required = reqwest::Client::new()
            .get(format!("http://{address}/round-trip?page=1"))
            .send()
            .await
            .unwrap();
        assert_eq!(missing_required.status(), reqwest::StatusCode::BAD_REQUEST);

        let comma_error = client
            .query_round_trip(
                1,
                None,
                QueryRoundTripRequiredExpanded::default(),
                None,
                QueryRoundTripCompact {
                    kind: Some("public,private".into()),
                    count: None,
                },
                None,
                vec![1],
                None,
            )
            .await
            .unwrap_err();
        match comma_error {
            ApiOpError::Transport(HttpError::Serialization(message)) => {
                assert!(message.contains("use explode=true"));
            }
            other => panic!("unexpected comma serialization error: {other:?}"),
        }

        server.abort();
    }
}
"#,
    )
    .expect("scratch lib");

    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/server-query-roundtrip-smoke"),
        )
        .output()
        .expect("scratch cargo test");
    assert!(
        output.status.success(),
        "generated client/server round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn bad_query_spec(parameter: Value, extra_parameter: Option<Value>) -> Value {
    let mut parameters = vec![parameter];
    parameters.extend(extra_parameter);
    json!({
        "openapi": "3.1.0",
        "info": { "title": "bad query", "version": "1.0.0" },
        "paths": {
            "/bad": { "get": {
                "operationId": "badQuery",
                "parameters": parameters,
                "responses": { "204": { "description": "unused" } }
            }}
        }
    })
}

fn server_generation_error(spec: Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer");
    let analysis = analyzer.analyze().expect("analysis");
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["badQuery".into()],
        prune_models: false,
    };
    let config = GeneratorConfig {
        server: Some(server.clone()),
        ..Default::default()
    };
    ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .expect_err("unsupported query shape must fail server generation")
        .to_string()
}

#[test]
fn unsupported_query_wire_shapes_fail_server_generation_with_context() {
    let deep_no_explode = server_generation_error(bad_query_spec(
        json!({
            "name": "filter",
            "in": "query",
            "style": "deepObject",
            "explode": false,
            "schema": { "type": "object", "properties": { "name": { "type": "string" } } }
        }),
        None,
    ));
    assert!(deep_no_explode.contains("badQuery"));
    assert!(deep_no_explode.contains("filter"));
    assert!(deep_no_explode.contains("deepObject"));

    let array_objects = server_generation_error(bad_query_spec(
        json!({
            "name": "filters",
            "in": "query",
            "schema": {
                "type": "array",
                "items": { "type": "object", "properties": { "name": { "type": "string" } } }
            }
        }),
        None,
    ));
    assert!(array_objects.contains("filters"));
    assert!(array_objects.contains("array"));

    let nested_object = server_generation_error(bad_query_spec(
        json!({
            "name": "filter",
            "in": "query",
            "schema": {
                "type": "object",
                "properties": {
                    "nested": { "type": "object", "properties": { "name": { "type": "string" } } }
                }
            }
        }),
        None,
    ));
    assert!(nested_object.contains("nested"));
    assert!(nested_object.contains("not scalar"));

    let mut composed_spec = bad_query_spec(
        json!({
            "name": "filter",
            "in": "query",
            "schema": { "$ref": "#/components/schemas/ComposedFilter" }
        }),
        None,
    );
    composed_spec["components"] = json!({
        "schemas": {
            "FilterPart": {
                "type": "object",
                "properties": { "name": { "type": "string" } }
            },
            "FilterOther": {
                "type": "object",
                "properties": { "active": { "type": "boolean" } }
            },
            "ComposedFilter": {
                "oneOf": [
                    { "$ref": "#/components/schemas/FilterPart" },
                    { "$ref": "#/components/schemas/FilterOther" }
                ]
            }
        }
    });
    let composed = server_generation_error(composed_spec);
    assert!(composed.contains("composed or union query schemas"));
}

#[test]
fn form_exploded_object_property_collisions_fail_server_generation() {
    let error = server_generation_error(bad_query_spec(
        json!({
            "name": "filter",
            "in": "query",
            "schema": {
                "type": "object",
                "properties": { "page": { "type": "integer" } }
            }
        }),
        Some(json!({
            "name": "page",
            "in": "query",
            "schema": { "type": "integer" }
        })),
    ));
    assert!(error.contains("badQuery"));
    assert!(error.contains("wire key `page`"));
    assert!(error.contains("filter"));

    let deep_error = server_generation_error(bad_query_spec(
        json!({
            "name": "filter",
            "in": "query",
            "style": "deepObject",
            "schema": {
                "type": "object",
                "properties": { "page": { "type": "integer" } }
            }
        }),
        Some(json!({
            "name": "filter[page]",
            "in": "query",
            "schema": { "type": "integer" }
        })),
    ));
    assert!(deep_error.contains("wire key `filter[page]`"));
}
