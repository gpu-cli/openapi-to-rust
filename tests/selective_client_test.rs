use openapi_to_rust::config::{ClientSection, ServerSection};
use openapi_to_rust::server::codegen::ServerCodegen;
use openapi_to_rust::streaming::{StreamingConfig, StreamingEndpoint};
use openapi_to_rust::{CodeGenerator, ConfigFile, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::collections::BTreeSet;
use std::process::Command;

fn selection_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "selection", "version": "1.0.0" },
        "paths": {
            "/users/{id}": {
                "get": {
                    "operationId": "getUser",
                    "tags": ["Users"],
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string" } },
                        { "name": "mode", "in": "query", "schema": { "type": "string", "enum": ["short", "full"] } }
                    ],
                    "responses": {
                        "200": { "description": "ok", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/User" } } } },
                        "400": { "description": "bad", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/GetUserError" } } } }
                    }
                }
            },
            "/users": {
                "post": {
                    "operationId": "createUser",
                    "tags": ["Users"],
                    "responses": {
                        "201": { "description": "created", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/User" } } } },
                        "422": { "description": "bad", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateUserError" } } } }
                    }
                }
            },
            "/admin": {
                "get": {
                    "operationId": "getAdmin",
                    "tags": ["Admin"],
                    "responses": {
                        "200": { "description": "ok", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Admin" } } } }
                    }
                }
            }
        },
        "components": { "schemas": {
            "User": { "type": "object", "properties": { "name": { "type": "string" } } },
            "Admin": { "type": "object", "properties": { "name": { "type": "string" } } },
            "GetUserError": { "type": "object", "properties": { "message": { "type": "string" } } },
            "CreateUserError": { "type": "object", "properties": { "message": { "type": "string" } } },
            "UnusedIsland": { "type": "object", "properties": { "value": { "type": "string" } } }
        }}
    })
}

fn collision_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "duplicates", "version": "1.0.0" },
        "paths": {
            "/first": { "get": {
                "operationId": "foo",
                "responses": { "200": { "description": "ok" } }
            }},
            "/second": { "post": {
                "operationId": "foo",
                "responses": { "200": { "description": "ok" } }
            }},
            "/zzz-case": { "put": {
                "operationId": "Foo",
                "requestBody": { "required": true, "content": { "application/json": {
                    "schema": { "$ref": "#/components/schemas/Event" }
                }}},
                "responses": { "200": { "description": "ok" } }
            }}
        },
        "components": { "schemas": {
            "Event": { "type": "object", "properties": { "value": { "type": "string" } } }
        }}
    })
}

fn custom_method_spec() -> serde_json::Value {
    json!({
        "openapi": "3.2.0",
        "info": { "title": "extension methods", "version": "1.0.0" },
        "paths": {
            "/things": {
                "get": {
                    "operationId": "listThings",
                    "responses": { "200": { "description": "ok" } }
                },
                "query": {
                    "operationId": "queryThings",
                    "tags": ["Things"],
                    "responses": { "204": { "description": "queried" } }
                },
                "additionalOperations": {
                    "PURGE": {
                        "operationId": "purgeThings",
                        "tags": ["Things"],
                        "responses": { "204": { "description": "purged" } }
                    }
                }
            }
        }
    })
}

fn generate_client(selectors: Option<Vec<&str>>) -> String {
    let mut analyzer = SchemaAnalyzer::new(selection_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let client = selectors.map(|operations| ClientSection {
        operations: operations.into_iter().map(str::to_string).collect(),
        prune_models: false,
    });
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        client,
        ..Default::default()
    });
    generator
        .generate_all(&mut analysis)
        .unwrap()
        .files
        .into_iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap()
        .content
}

#[test]
fn absent_or_empty_client_scope_generates_all_operations() {
    for selectors in [None, Some(Vec::new())] {
        let client = generate_client(selectors);
        assert!(client.contains("pub async fn get_user"));
        assert!(client.contains("pub async fn create_user"));
        assert!(client.contains("pub async fn get_admin"));
    }
}

#[test]
fn operation_id_method_path_and_tag_selectors_share_one_grammar() {
    let by_id = generate_client(Some(vec!["getUser"]));
    assert!(by_id.contains("pub async fn get_user"));
    assert!(!by_id.contains("pub async fn create_user"));

    let by_path = generate_client(Some(vec!["POST /users"]));
    assert!(by_path.contains("pub async fn create_user"));
    assert!(!by_path.contains("pub async fn get_user"));

    let by_tag = generate_client(Some(vec!["tag:Users"]));
    assert!(by_tag.contains("pub async fn get_user"));
    assert!(by_tag.contains("pub async fn create_user"));
    assert!(!by_tag.contains("pub async fn get_admin"));
}

#[test]
fn selection_filters_parameter_enums_and_operation_error_enums_together() {
    let client = generate_client(Some(vec!["getUser"]));
    assert!(client.contains("pub enum GetUserMode"));
    assert!(client.contains("pub enum GetUserApiError"));
    assert!(!client.contains("pub enum CreateUserApiError"));
}

#[test]
fn client_resolution_errors_cover_unknown_ambiguous_and_renamed_ids() {
    let mut analyzer = SchemaAnalyzer::new(selection_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let unknown = CodeGenerator::new(GeneratorConfig {
        client: Some(ClientSection {
            operations: vec!["getUsr".into()],
            prune_models: false,
        }),
        ..Default::default()
    })
    .generate_all(&mut analysis)
    .unwrap_err()
    .to_string();
    assert!(unknown.contains("Did you mean `getUser`"));

    let mut analyzer = SchemaAnalyzer::new(collision_spec()).unwrap();
    let analysis = analyzer.analyze().unwrap();

    let error_for = |selector: &str| {
        let mut analysis = analysis.clone();
        CodeGenerator::new(GeneratorConfig {
            client: Some(ClientSection {
                operations: vec![selector.to_string()],
                prune_models: false,
            }),
            ..Default::default()
        })
        .generate_all(&mut analysis)
        .unwrap_err()
        .to_string()
    };
    let ambiguous = error_for("foo");
    assert!(ambiguous.contains("ambiguous"));
    assert!(ambiguous.contains("foo_post"));
    assert!(ambiguous.contains("METHOD /path"));

    let renamed = error_for("Foo");
    assert!(renamed.contains("was renamed"));
    assert!(renamed.contains("Foo_put"));

    let mut escaped_analysis = analysis.clone();
    let result = CodeGenerator::new(GeneratorConfig {
        client: Some(ClientSection {
            operations: vec!["GET /first".into(), "foo_post".into(), "Foo_put".into()],
            prune_models: false,
        }),
        ..Default::default()
    })
    .generate_all(&mut escaped_analysis)
    .unwrap();
    let client = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap();
    assert!(client.content.contains("pub async fn foo("));
    assert!(client.content.contains("pub async fn foo_post("));
    assert!(client.content.contains("pub async fn foo_put("));
}

#[test]
fn config_client_section_is_strict_and_validates_selector_syntax() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join("openapi.json"), r#"{"openapi":"3.1.0"}"#).unwrap();
    let config_path = dir.path().join("openapi-to-rust.toml");
    std::fs::write(
        &config_path,
        r#"[generator]
spec_path = "openapi.json"
output_dir = "generated"
module_name = "api"

[features]
enable_async_client = true

[client]
operations = ["getUser", "POST /users", "tag:Users"]
prune_models = true
"#,
    )
    .unwrap();
    let mut config = ConfigFile::load(&config_path).unwrap();
    let client = config.client.as_ref().unwrap();
    assert_eq!(client.operations.len(), 3);
    assert!(client.prune_models);

    let serialized = toml::to_string_pretty(&config).unwrap();
    let reparsed: ConfigFile = toml::from_str(&serialized).unwrap();
    assert_eq!(
        reparsed.client.as_ref().unwrap().operations,
        client.operations
    );
    assert!(reparsed.client.as_ref().unwrap().prune_models);

    config.client.as_mut().unwrap().operations.clear();
    let serialized_empty = toml::to_string_pretty(&config).unwrap();
    let reparsed_empty: ConfigFile = toml::from_str(&serialized_empty).unwrap();
    assert!(reparsed_empty.client.unwrap().operations.is_empty());

    let invalid = std::fs::read_to_string(&config_path)
        .unwrap()
        .replace("prune_models = true", "unknown = true");
    std::fs::write(&config_path, invalid).unwrap();
    assert!(
        ConfigFile::load(&config_path)
            .unwrap_err()
            .to_string()
            .contains("unknown field `unknown`")
    );

    let malformed = std::fs::read_to_string(&config_path)
        .unwrap()
        .replace("unknown = true", "prune_models = true")
        .replace("getUser", "not a selector");
    std::fs::write(&config_path, malformed).unwrap();
    let error = ConfigFile::load(&config_path).unwrap_err().to_string();
    assert!(error.contains("client.operations[0]"));
    assert!(error.contains("METHOD /path"));
}

#[test]
fn empty_client_scope_prunes_only_schemas_unreachable_from_all_operations() {
    let mut analyzer = SchemaAnalyzer::new(selection_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let result = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: Vec::new(),
            prune_models: true,
        }),
        ..Default::default()
    })
    .generate_all(&mut analysis)
    .unwrap();

    assert!(analysis.schemas.contains_key("User"));
    assert!(analysis.schemas.contains_key("Admin"));
    assert!(analysis.schemas.contains_key("GetUserError"));
    assert!(analysis.schemas.contains_key("CreateUserError"));
    assert!(!analysis.schemas.contains_key("UnusedIsland"));
    assert_eq!(result.pruned_schemas, 1);
    let client = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap();
    assert!(client.content.contains("pub async fn get_user"));
    assert!(client.content.contains("pub async fn create_user"));
    assert!(client.content.contains("pub async fn get_admin"));
}

#[test]
fn disabled_client_scope_is_ignored_and_registry_keeps_all_operations() {
    let mut analyzer = SchemaAnalyzer::new(selection_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let result = CodeGenerator::new(GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        enable_registry: true,
        client: Some(ClientSection {
            operations: vec!["doesNotExist".into()],
            prune_models: true,
        }),
        ..Default::default()
    })
    .generate_all(&mut analysis)
    .unwrap();

    assert_eq!(result.pruned_schemas, 0);
    assert!(
        result
            .files
            .iter()
            .all(|file| file.path != std::path::Path::new("client.rs"))
    );
    let registry = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("registry.rs"))
        .unwrap();
    assert!(registry.content.contains("getUser"));
    assert!(registry.content.contains("createUser"));
    assert!(registry.content.contains("getAdmin"));
}

#[test]
fn registry_only_pruning_does_not_reintroduce_an_async_client_scope() {
    let mut analyzer = SchemaAnalyzer::new(selection_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let result = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_registry: true,
        registry_only: true,
        client: Some(ClientSection {
            operations: vec!["doesNotExist".into()],
            prune_models: true,
        }),
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["getUser".into()],
            prune_models: true,
        }),
        ..Default::default()
    })
    .generate_all(&mut analysis)
    .unwrap();

    assert!(!analysis.schemas.contains_key("Admin"));
    assert!(!analysis.schemas.contains_key("UnusedIsland"));
    assert!(
        result
            .files
            .iter()
            .all(|file| file.path == std::path::Path::new("registry.rs"))
    );
    let registry = &result.files[0].content;
    assert!(registry.contains("getUser"));
    assert!(registry.contains("getAdmin"));
}

#[test]
fn selective_client_does_not_make_custom_method_registry_lossy() {
    let mut analyzer = SchemaAnalyzer::new(custom_method_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let result = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_registry: true,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec!["GET /things".into()],
            prune_models: false,
        }),
        ..Default::default()
    })
    .generate_all(&mut analysis)
    .unwrap();

    let client = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap();
    assert!(client.content.contains("pub async fn list_things"));
    assert!(!client.content.contains("pub async fn purge_things"));

    let registry = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("registry.rs"))
        .unwrap();
    assert!(registry.content.contains("purgeThings"));
    assert!(registry.content.contains("method: HttpMethod::Query"));
    assert!(registry.content.contains("Self::Custom0 => \"PURGE\""));
    assert!(registry.content.contains("method: HttpMethod::Custom0"));
}

#[test]
fn generated_custom_method_server_route_compiles_with_exact_guard() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut analyzer = SchemaAnalyzer::new(custom_method_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["QUERY /things".into(), "PURGE /things".into()],
        prune_models: true,
    };
    let config = GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "custom".into(),
        enable_async_client: false,
        server: Some(server.clone()),
        ..Default::default()
    };
    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();
    let server_files = ServerCodegen::new(generator.config(), &analysis, &server)
        .generate()
        .unwrap();
    for file in server_files {
        let path = output_dir.join(file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, file.content).unwrap();
    }
    std::fs::write(
        output_dir.join("mod.rs"),
        "pub mod types;\npub use types::*;\npub mod server;\npub use server::*;\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::server::*;
    use axum::{body::Body, http::{Method, Request, StatusCode}};
    use tower::ServiceExt;

    #[derive(Clone)]
    struct Api;

    #[axum::async_trait]
    impl ThingsApi for Api {
        async fn query_things(&self) -> QueryThingsResponse {
            QueryThingsResponse::Empty
        }

        async fn purge_things(&self) -> PurgeThingsResponse {
            PurgeThingsResponse::Empty
        }
    }

    #[tokio::test]
    async fn custom_methods_dispatch_exactly_without_router_conflicts() {
        let app = things_api_router(Api);
        for method in ["QUERY", "PURGE"] {
            let request = Request::builder()
                .method(Method::from_bytes(method.as_bytes()).unwrap())
                .uri("/things")
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }

        let request = Request::builder()
            .method(Method::POST)
            .uri("/things")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "custom-method-server"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.7"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
tower = { version = "0.5", features = ["util"] }
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/custom-method-server-smoke"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "custom method server failed to compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let router = std::fs::read_to_string(output_dir.join("server/router.rs")).unwrap();
    assert_eq!(router.matches("routing::any").count(), 1);
    assert!(router.contains("\"QUERY\" =>"));
    assert!(router.contains("\"PURGE\" =>"));
    assert!(router.contains("StatusCode::METHOD_NOT_ALLOWED"));
}

#[test]
fn cross_tag_custom_methods_on_one_path_fail_during_generation() {
    let mut spec = custom_method_spec();
    spec["paths"]["/things"]["query"]["tags"] = json!(["Queries"]);
    spec["paths"]["/things"]["additionalOperations"]["PURGE"]["tags"] = json!(["Administration"]);
    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let analysis = analyzer.analyze().unwrap();
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["QUERY /things".into(), "PURGE /things".into()],
        prune_models: false,
    };
    let config = GeneratorConfig {
        enable_async_client: false,
        server: Some(server.clone()),
        ..Default::default()
    };
    let error = ServerCodegen::new(&config, &analysis, &server)
        .generate()
        .unwrap_err()
        .to_string();
    assert!(error.contains("custom HTTP methods on `/things`"));
    assert!(error.contains("Administration"));
    assert!(error.contains("Queries"));
    assert!(error.contains("same first tag"));
}

#[test]
fn standalone_client_generation_does_not_validate_server_scope() {
    let mut analyzer = SchemaAnalyzer::new(selection_spec()).unwrap();
    let analysis = analyzer.analyze().unwrap();
    let client = CodeGenerator::new(GeneratorConfig {
        client: Some(ClientSection {
            operations: vec!["getUser".into()],
            prune_models: false,
        }),
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["doesNotExist".into()],
            prune_models: false,
        }),
        ..Default::default()
    })
    .generate_http_client(&analysis)
    .unwrap();

    assert!(client.contains("pub async fn get_user"));
    assert!(!client.contains("pub async fn create_user"));
    assert!(!client.contains("pub async fn get_admin"));
}

#[test]
fn streaming_operation_ids_use_alias_aware_resolution() {
    let mut analyzer = SchemaAnalyzer::new(collision_spec()).unwrap();
    let analysis = analyzer.analyze().unwrap();
    let endpoint = StreamingEndpoint {
        operation_id: "Foo".into(),
        path: "/zzz-case".into(),
        event_union_type: "Event".into(),
        ..Default::default()
    };
    let config_for = |endpoint: StreamingEndpoint| GeneratorConfig {
        enable_async_client: true,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec!["GET /first".into()],
            prune_models: true,
        }),
        streaming_config: Some(StreamingConfig {
            endpoints: vec![endpoint],
            generate_client: false,
            event_parser_helpers: false,
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut renamed_analysis = analysis.clone();
    let error = CodeGenerator::new(config_for(endpoint.clone()))
        .generate_all(&mut renamed_analysis)
        .unwrap_err()
        .to_string();
    assert!(error.contains("[streaming].endpoints[0].operation_id"));
    assert!(error.contains("renamed to `Foo_put`"));

    let mut selector_endpoint = endpoint.clone();
    selector_endpoint.operation_id = "PUT /zzz-case".into();
    let mut selector_analysis = analysis.clone();
    let error = CodeGenerator::new(config_for(selector_endpoint))
        .generate_all(&mut selector_analysis)
        .unwrap_err()
        .to_string();
    assert!(error.contains("[streaming].endpoints[0].operation_id"));
    assert!(error.contains("no operation with id `PUT /zzz-case`"));

    let mut canonical_endpoint = endpoint;
    canonical_endpoint.operation_id = "Foo_put".into();
    let mut canonical_analysis = analysis;
    let result = CodeGenerator::new(config_for(canonical_endpoint))
        .generate_all(&mut canonical_analysis)
        .unwrap();
    assert!(canonical_analysis.schemas.contains_key("Event"));
    let streaming = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("streaming.rs"))
        .unwrap();
    assert!(streaming.content.contains("request: Event"));
}

#[test]
fn model_pruning_keeps_union_of_client_and_server_reachability() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "union", "version": "1.0.0" },
        "paths": {
            "/call": { "post": {
                "operationId": "callRemote",
                "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ClientRoot" } } } },
                "responses": { "200": { "description": "ok" } }
            }},
            "/host": { "post": {
                "operationId": "hostLocal",
                "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ServerRoot" } } } },
                "responses": { "200": { "description": "ok" } }
            }},
            "/unused": { "get": {
                "operationId": "unused",
                "responses": { "200": { "description": "ok", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/UnusedRoot" } } } } }
            }}
        },
        "components": { "schemas": {
            "ClientRoot": { "type": "object", "properties": { "shared": { "$ref": "#/components/schemas/Shared" } } },
            "ServerRoot": { "type": "object", "properties": { "shared": { "$ref": "#/components/schemas/Shared" } } },
            "Shared": { "type": "object", "properties": { "value": { "type": "string" } } },
            "UnusedRoot": { "type": "object", "properties": { "value": { "type": "string" } } },
            "UnusedIsland": { "type": "object", "properties": { "value": { "type": "string" } } }
        }}
    });
    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec!["callRemote".into()],
            prune_models: true,
        }),
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["hostLocal".into()],
            prune_models: false,
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    let names: BTreeSet<_> = analysis.schemas.keys().map(String::as_str).collect();
    assert!(names.contains("ClientRoot"));
    assert!(names.contains("ServerRoot"));
    assert!(names.contains("Shared"));
    assert!(!names.contains("UnusedRoot"));
    assert!(!names.contains("UnusedIsland"));
    assert_eq!(result.pruned_schemas, 2);
}

#[test]
fn server_pruning_preserves_every_operation_for_an_unscoped_client() {
    let mut analyzer = SchemaAnalyzer::new(selection_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        client: None,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["getUser".into()],
            prune_models: true,
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    assert!(analysis.schemas.contains_key("Admin"));
    assert!(analysis.schemas.contains_key("CreateUserError"));
    let client = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap();
    assert!(client.content.contains("pub async fn get_admin"));
    assert!(client.content.contains("pub async fn create_user"));
}

#[test]
fn selected_operation_retains_owned_inline_schemas_only() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "synthetics", "version": "1.0.0" },
        "paths": {
            "/selected": { "post": {
                "operationId": "selectedOp",
                "requestBody": { "content": { "application/json": { "schema": {
                    "type": "object",
                    "properties": { "kind": { "type": "string", "enum": ["a", "b"] } }
                } } } },
                "responses": { "200": { "description": "ok" } }
            }},
            "/unselected": { "post": {
                "operationId": "unselectedOp",
                "requestBody": { "content": { "application/json": { "schema": {
                    "type": "object",
                    "properties": { "kind": { "type": "string", "enum": ["x", "y"] } }
                } } } },
                "responses": { "200": { "description": "ok" } }
            }}
        }
    });
    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let before: BTreeSet<_> = analysis.schemas.keys().cloned().collect();
    assert!(before.iter().any(|name| name.starts_with("SelectedOp")));
    assert!(before.iter().any(|name| name.starts_with("UnselectedOp")));

    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec!["selectedOp".into()],
            prune_models: true,
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    let names: BTreeSet<_> = analysis.schemas.keys().cloned().collect();
    assert!(names.iter().any(|name| name.starts_with("SelectedOp")));
    assert!(!names.iter().any(|name| name.starts_with("UnselectedOp")));
    let types = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("types.rs"))
        .unwrap();
    assert!(types.content.contains("pub struct SelectedOpRequest"));
}

#[test]
fn selective_openai_client_compiles() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let spec: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("tests/fixtures/openai-responses.json"))
            .unwrap(),
    )
    .unwrap();
    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let schema_count_before_pruning = analysis.schemas.len();
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "openai".into(),
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec!["createResponse".into()],
            prune_models: true,
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    assert!(result.pruned_schemas > 0);
    assert!(analysis.schemas.len() < schema_count_before_pruning);
    generator.write_files(&result).unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), "pub mod generated;\n").unwrap();

    let generated_dependencies = result
        .required_deps
        .iter()
        .map(|dependency| dependency.to_toml_line())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "selective-openai-client"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
thiserror = "2"
reqwest = {{ version = "0.12", features = ["json", "multipart"] }}
reqwest-middleware = {{ version = "0.4", features = ["multipart"] }}
{generated_dependencies}
"#
        ),
    )
    .unwrap();

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/selective-client-smoke"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "selective OpenAI client failed to compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
