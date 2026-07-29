use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::streaming::{ReconnectionConfig, StreamingConfig, StreamingEndpoint};
use openapi_to_rust::type_mapping::{BinaryStrategy, DurationStrategy, TypeMappingConfig};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, RetryConfig, SchemaAnalyzer, TypeMapper};
use serde_json::json;
use std::collections::BTreeSet;
use std::process::Command;

fn requirements_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "requirements", "version": "1.0.0" },
        "paths": {
            "/items": {
                "get": {
                    "operationId": "listItems",
                    "tags": ["Items"],
                    "parameters": [{
                        "name": "filter",
                        "in": "query",
                        "required": true,
                        "style": "deepObject",
                        "explode": true,
                        "schema": { "$ref": "#/components/schemas/QueryFilter" }
                    }],
                    "responses": {
                        "200": {
                            "description": "ok",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/Payload" }
                                }
                            }
                        }
                    }
                }
            },
            "/form": {
                "post": {
                    "operationId": "submitForm",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/x-www-form-urlencoded": {
                                "schema": { "$ref": "#/components/schemas/Payload" }
                            }
                        }
                    },
                    "responses": { "204": { "description": "ok" } }
                }
            },
            "/upload/{id}.json": {
                "post": {
                    "operationId": "uploadPayload",
                    "parameters": [{
                        "name": "id",
                        "in": "path",
                        "required": true,
                        "schema": { "type": "string" }
                    }],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "multipart/form-data": {
                                "schema": { "$ref": "#/components/schemas/Payload" }
                            }
                        }
                    },
                    "responses": { "204": { "description": "ok" } }
                }
            },
            "/stream": {
                "post": {
                    "operationId": "streamItems",
                    "tags": ["Items"],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Payload" }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "events",
                            "content": {
                                "text/event-stream": {
                                    "schema": { "$ref": "#/components/schemas/StreamEvent" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Payload": {
                    "type": "object",
                    "required": ["id", "happened_at", "elapsed", "encoded", "raw", "resource"],
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "happened_at": { "type": "string", "format": "date-time" },
                        "elapsed": { "type": "string", "format": "duration" },
                        "encoded": { "type": "string", "format": "byte" },
                        "raw": { "type": "string", "format": "binary" },
                        "resource": { "type": "string", "format": "uri" }
                    }
                },
                "QueryFilter": {
                    "type": "object",
                    "required": ["limit"],
                    "properties": {
                        "limit": { "type": "integer", "format": "int32" },
                        "active": { "type": "boolean" }
                    }
                },
                "StreamEvent": {
                    "type": "object",
                    "required": ["message"],
                    "properties": { "message": { "type": "string" } }
                }
            }
        }
    })
}

fn dependency_names(result: &openapi_to_rust::GenerationResult) -> BTreeSet<&'static str> {
    result
        .required_deps
        .iter()
        .map(|dependency| dependency.crate_name)
        .collect()
}

fn assert_names(
    result: &openapi_to_rust::GenerationResult,
    expected: impl IntoIterator<Item = &'static str>,
) {
    assert_eq!(dependency_names(result), expected.into_iter().collect());
}

fn assert_version(result: &openapi_to_rust::GenerationResult, crate_name: &str, expected: &str) {
    let dependency = result
        .required_deps
        .iter()
        .find(|dependency| dependency.crate_name == crate_name)
        .unwrap_or_else(|| panic!("missing {crate_name} dependency"));
    assert_eq!(
        dependency.version, expected,
        "unexpected {crate_name} version"
    );
}

fn compile_case(name: &str, mut config: GeneratorConfig) -> openapi_to_rust::GenerationResult {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::TempDir::new().expect("scratch crate");
    let output_dir = temp.path().join("src/generated");
    config.output_dir = output_dir.clone();
    config.module_name = name.to_string();

    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    let package = format!(
        "[package]\nname = \"generated-requirements-{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n"
    );
    let manifest_path = temp.path().join("Cargo.toml");
    std::fs::write(&manifest_path, &package).expect("sentinel manifest");

    let mut analyzer = SchemaAnalyzer::with_type_mapper(
        requirements_spec(),
        TypeMapper::new(config.types.clone()),
    )
    .expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(config);
    let result = generator
        .generate_all(&mut analysis)
        .expect("generation succeeds");
    generator
        .write_files(&result)
        .expect("generated files write");

    assert_eq!(
        std::fs::read_to_string(&manifest_path).expect("sentinel manifest reads"),
        package,
        "generation must not modify the consuming Cargo.toml"
    );
    let dependency_fragment = std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml"))
        .expect("complete dependency fragment");
    let parsed = dependency_fragment
        .parse::<toml::Table>()
        .expect("dependency fragment is valid TOML");
    assert_eq!(
        parsed
            .get("dependencies")
            .and_then(toml::Value::as_table)
            .map(toml::Table::len),
        Some(result.required_deps.len())
    );

    std::fs::write(&manifest_path, format!("{package}\n{dependency_fragment}"))
        .expect("scratch manifest");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "#![allow(dead_code, unused_imports)]\npub mod generated;\n",
    )
    .expect("scratch lib");
    let output = Command::new("cargo")
        .args(["check", "--lib", "--quiet"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/generated-requirements"),
        )
        .output()
        .expect("cargo check runs");
    assert!(
        output.status.success(),
        "{name} failed to compile from REQUIRED_DEPS.toml only:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    result
}

fn streaming_config() -> StreamingConfig {
    StreamingConfig {
        endpoints: vec![StreamingEndpoint {
            operation_id: "streamItems".into(),
            path: "/stream".into(),
            stream_parameter: "stream".into(),
            event_union_type: "StreamEvent".into(),
            ..Default::default()
        }],
        reconnection_config: Some(ReconnectionConfig::default()),
        ..Default::default()
    }
}

fn server_config() -> ServerSection {
    ServerSection {
        framework: "axum".into(),
        operations: vec!["listItems".into(), "streamItems".into()],
        prune_models: false,
        validation: Default::default(),
    }
}

fn plain_server_config() -> ServerSection {
    ServerSection {
        framework: "axum".into(),
        operations: vec!["listItems".into()],
        prune_models: false,
        validation: Default::default(),
    }
}

#[test]
fn disabled_sse_feature_does_not_emit_streaming_code_or_dependencies() {
    let config = GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        streaming_config: Some(streaming_config()),
        tracing_enabled: false,
        ..Default::default()
    };
    let mut analyzer = SchemaAnalyzer::with_type_mapper(
        requirements_spec(),
        TypeMapper::new(config.types.clone()),
    )
    .expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analysis");
    let result = CodeGenerator::new(config)
        .generate_all(&mut analysis)
        .expect("generation");

    assert!(
        result
            .files
            .iter()
            .all(|file| file.path != std::path::Path::new("streaming.rs"))
    );
    assert!(
        result
            .files
            .iter()
            .all(|file| file.path != std::path::Path::new("sse.rs"))
    );
    assert!(!dependency_names(&result).contains("reqwest-eventsource"));
    assert!(!dependency_names(&result).contains("futures-util"));
}

#[test]
fn multipart_server_enables_axum_multipart_feature() {
    let result = compile_case(
        "multipart-server",
        GeneratorConfig {
            enable_async_client: false,
            enable_sse_client: false,
            tracing_enabled: false,
            server: Some(ServerSection {
                framework: "axum".into(),
                operations: vec!["uploadPayload".into()],
                prune_models: false,
                validation: Default::default(),
            }),
            ..Default::default()
        },
    );
    let axum = result
        .required_deps
        .iter()
        .find(|dependency| dependency.crate_name == "axum")
        .expect("axum dependency");
    assert_eq!(axum.features, vec!["json", "multipart"]);
}

#[test]
fn multipart_client_and_server_compile_for_every_binary_strategy() {
    for (name, binary) in [
        ("multipart-binary-bytes", BinaryStrategy::Bytes),
        ("multipart-binary-vec", BinaryStrategy::VecU8),
        ("multipart-binary-string", BinaryStrategy::String),
    ] {
        compile_case(
            name,
            GeneratorConfig {
                enable_sse_client: false,
                tracing_enabled: false,
                types: TypeMappingConfig {
                    binary,
                    ..Default::default()
                },
                server: Some(ServerSection {
                    framework: "axum".into(),
                    operations: vec!["uploadPayload".into()],
                    prune_models: false,
                    validation: ServerValidationSection {
                        enabled: false,
                        ..Default::default()
                    },
                }),
                ..Default::default()
            },
        );
    }
}

#[test]
fn every_generation_mode_compiles_from_its_exact_dependency_fragment() {
    let types = compile_case(
        "types",
        GeneratorConfig {
            enable_async_client: false,
            enable_sse_client: false,
            tracing_enabled: false,
            types: TypeMappingConfig {
                duration: DurationStrategy::Iso8601,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert_names(
        &types,
        [
            "base64", "bytes", "chrono", "iso8601", "serde", "url", "uuid",
        ],
    );
    let uuid = types
        .required_deps
        .iter()
        .find(|dependency| dependency.crate_name == "uuid")
        .expect("uuid dependency");
    assert_eq!(uuid.features, vec!["serde"]);

    let client = compile_case(
        "client",
        GeneratorConfig {
            enable_async_client: true,
            enable_sse_client: false,
            tracing_enabled: false,
            ..Default::default()
        },
    );
    assert_names(
        &client,
        [
            "base64",
            "bytes",
            "chrono",
            // The fixture's streaming operation declares only
            // `text/event-stream`, so its client method now returns a
            // `futures_util::Stream` of bytes rather than `()`
            // (openapi-generator-x9v).
            "futures-util",
            "reqwest",
            "reqwest-middleware",
            "serde",
            "serde_json",
            "serde_urlencoded",
            "thiserror",
            "url",
            "uuid",
        ],
    );
    let middleware = client
        .required_deps
        .iter()
        .find(|dependency| dependency.crate_name == "reqwest-middleware")
        .expect("middleware dependency");
    assert_eq!(middleware.version, "0.5");
    assert_eq!(middleware.features, vec!["multipart", "query"]);

    let sse = compile_case(
        "sse",
        GeneratorConfig {
            enable_async_client: false,
            enable_sse_client: true,
            streaming_config: Some(streaming_config()),
            tracing_enabled: false,
            ..Default::default()
        },
    );
    assert_names(
        &sse,
        [
            "async-trait",
            "base64",
            "bytes",
            "chrono",
            "futures-timer",
            "futures-util",
            "reqwest",
            "serde",
            "serde_json",
            "thiserror",
            "tracing",
            "url",
            "uuid",
        ],
    );
    let reqwest = sse
        .required_deps
        .iter()
        .find(|dependency| dependency.crate_name == "reqwest")
        .expect("reqwest dependency");
    assert_eq!(reqwest.version, "0.13");
    assert_eq!(reqwest.features, vec!["json", "rustls", "stream"]);
    assert!(!reqwest.default_features);
    assert_version(&sse, "thiserror", "2");
    let sse_runtime = sse
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("sse.rs"))
        .expect("SSE runtime module");
    assert!(sse_runtime.content.contains("pub struct SseClient"));
    assert!(sse_runtime.content.contains("pub struct SseEvent<T>"));
    assert!(
        sse_runtime
            .content
            .contains("pub struct SseReconnectOptions")
    );
    assert!(sse_runtime.content.contains("pub async fn stream_raw"));
    assert!(sse_runtime.content.contains("Last-Event-ID"));
    let streaming = sse
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("streaming.rs"))
        .expect("API-specific streaming module");
    assert!(
        streaming
            .content
            .contains("use super::sse::{SseClient, SseReconnectOptions}")
    );
    assert!(streaming.content.contains("with_reconnect_options"));
    assert!(sse.mod_file.content.contains("pub mod sse;"));
    assert!(!sse.mod_file.content.contains("pub use sse::*;"));

    let server = compile_case(
        "server",
        GeneratorConfig {
            enable_async_client: false,
            enable_sse_client: false,
            tracing_enabled: false,
            server: Some(plain_server_config()),
            ..Default::default()
        },
    );
    assert_names(
        &server,
        [
            "async-trait",
            "axum",
            "base64",
            "bytes",
            "chrono",
            "http-body-util",
            "jsonschema",
            "mime",
            "serde",
            "serde_json",
            "serde_urlencoded",
            "url",
            "uuid",
        ],
    );
    let axum = server
        .required_deps
        .iter()
        .find(|dependency| dependency.crate_name == "axum")
        .expect("axum dependency");
    assert_eq!(axum.version, "0.8");
    assert_eq!(axum.features, vec!["json"]);
    assert!(!axum.default_features);
    let jsonschema = server
        .required_deps
        .iter()
        .find(|dependency| dependency.crate_name == "jsonschema")
        .expect("jsonschema dependency");
    assert_eq!(jsonschema.version, "0.49");
    assert!(jsonschema.features.is_empty());
    assert!(!jsonschema.default_features);

    let combined = compile_case(
        "combined",
        GeneratorConfig {
            enable_async_client: true,
            enable_sse_client: true,
            streaming_config: Some(streaming_config()),
            tracing_enabled: true,
            retry_config: Some(RetryConfig {
                max_retries: 3,
                initial_delay_ms: 100,
                max_delay_ms: 1_000,
            }),
            server: Some(server_config()),
            ..Default::default()
        },
    );
    assert_names(
        &combined,
        [
            "async-trait",
            "axum",
            "base64",
            "bytes",
            "chrono",
            "futures-core",
            "futures-timer",
            "futures-util",
            "http-body-util",
            "jsonschema",
            "mime",
            "reqwest",
            "reqwest-middleware",
            "reqwest-retry",
            "reqwest-tracing",
            "serde",
            "serde_json",
            "serde_urlencoded",
            "thiserror",
            "tracing",
            "url",
            "uuid",
        ],
    );
    assert_version(&combined, "reqwest", "0.13");
    assert_version(&combined, "reqwest-middleware", "0.5");
    assert_version(&combined, "reqwest-retry", "0.9");
    assert_version(&combined, "reqwest-tracing", "0.7");
    assert_version(&combined, "thiserror", "2");
}
