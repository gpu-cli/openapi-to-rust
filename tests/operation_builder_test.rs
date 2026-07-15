use openapi_to_rust::config::{BuildersSection, ClientSection};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn builder_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "operation builders", "version": "1.0.0" },
        "paths": {
            "/widgets/{id}": { "patch": {
                "operationId": "updateWidget",
                "parameters": [
                    { "name": "id", "in": "path", "required": true,
                      "schema": { "type": "string" } },
                    { "name": "search", "in": "query",
                      "schema": { "type": "string" } },
                    { "name": "limit", "in": "query",
                      "schema": { "type": "integer", "format": "int32" } },
                    { "name": "tags", "in": "query", "style": "form", "explode": true,
                      "schema": { "type": "array", "items": { "type": "string" } } },
                    { "name": "x-trace", "in": "header",
                      "schema": { "type": "string" } },
                    { "name": "tenant-id", "in": "header", "required": true,
                      "schema": { "type": "string" } }
                ],
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/UpdateWidgetRequest"
                    } } }
                },
                "responses": {
                    "200": { "description": "updated", "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/Widget" }
                    } } },
                    "400": { "description": "bad", "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/Problem" }
                    } } }
                }
            }},
            "/boundary": { "get": {
                "operationId": "boundary",
                "parameters": [
                    { "name": "one", "in": "query", "schema": { "type": "string" } },
                    { "name": "two", "in": "query", "schema": { "type": "string" } },
                    { "name": "three", "in": "query", "schema": { "type": "string" } },
                    { "name": "four", "in": "query", "schema": { "type": "string" } }
                ],
                "responses": { "204": { "description": "ok" } }
            }},
            "/profile": { "patch": {
                "operationId": "patchProfile",
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/PatchProfileRequest"
                    } } }
                },
                "responses": { "204": { "description": "patched" } }
            }},
            "/draft": { "patch": {
                "operationId": "saveDraft",
                "requestBody": {
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/PatchProfileRequest"
                    } } }
                },
                "responses": { "204": { "description": "saved" } }
            }},
            "/collision/{client}": { "get": {
                "operationId": "client",
                "parameters": [
                    { "name": "client", "in": "path", "required": false,
                      "schema": { "type": "string" } },
                    { "name": "one", "in": "query", "schema": { "type": "string" } },
                    { "name": "two", "in": "query", "schema": { "type": "string" } },
                    { "name": "three", "in": "query", "schema": { "type": "string" } },
                    { "name": "four", "in": "query", "schema": { "type": "string" } }
                ],
                "responses": { "204": { "description": "ok" } }
            }},
            "/composed": { "post": {
                "operationId": "createComposed",
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": {
                        "$ref": "#/components/schemas/ComposedRequest"
                    } } }
                },
                "responses": { "204": { "description": "created" } }
            }}
        },
        "components": { "schemas": {
            "UpdateWidgetRequest": {
                "type": "object",
                "additionalProperties": false,
                "required": ["name"],
                "properties": {
                    "name": { "type": "string" },
                    "instructions": { "type": "string" },
                    "send": { "type": "string" },
                    "with_send": { "type": "string" },
                    "request": { "type": "string" },
                    "connectionString": { "type": "string" },
                    "connection_string": { "type": "string" }
                }
            },
            "Widget": {
                "type": "object", "required": ["id"],
                "properties": { "id": { "type": "string" } }
            },
            "Problem": {
                "type": "object", "required": ["message"],
                "properties": { "message": { "type": "string" } }
            },
            "PatchProfileRequest": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "title": { "type": "string" },
                    "bio": { "type": "string" },
                    "active": { "type": "boolean" },
                    "color": { "type": "string" }
                }
            },
            "ComposedRequest": {
                "allOf": [
                    { "$ref": "#/components/schemas/ComposedPartA" },
                    { "$ref": "#/components/schemas/ComposedPartB" }
                ]
            },
            "ComposedPartA": {
                "type": "object", "properties": {
                    "alpha": { "type": "string" },
                    "beta": { "type": "boolean" }
                }
            },
            "ComposedPartB": {
                "type": "object", "properties": {
                    "gamma": { "type": "integer" },
                    "delta": { "type": "string" }
                }
            }
        } }
    })
}

fn generate(builders: BuildersSection) -> (String, String) {
    let mut analyzer = SchemaAnalyzer::new(builder_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let result = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        builders,
        ..Default::default()
    })
    .generate_all(&mut analysis)
    .unwrap();
    let file = |name: &str| {
        result
            .files
            .iter()
            .find(|file| file.path == std::path::Path::new(name))
            .unwrap()
            .content
            .clone()
    };
    (file("types.rs"), file("client.rs"))
}

#[test]
fn builders_are_disabled_by_default_and_use_a_strict_threshold() {
    let (_, disabled) = generate(BuildersSection::default());
    assert!(!disabled.contains("update_widget_builder"));
    assert!(!disabled.contains("boundary_builder"));

    let (_, at_boundary) = generate(BuildersSection {
        enabled: true,
        threshold: 4,
    });
    assert!(!at_boundary.contains("pub fn boundary_builder"));

    let (_, above_boundary) = generate(BuildersSection {
        enabled: true,
        threshold: 3,
    });
    assert!(above_boundary.contains("pub fn boundary_builder"));
}

#[test]
fn mixed_body_builder_is_additive_and_collision_safe() {
    let (_, client) = generate(BuildersSection {
        enabled: true,
        threshold: 3,
    });
    let compact = client.split_whitespace().collect::<String>();

    assert!(compact.contains("pubasyncfnupdate_widget("));
    assert!(compact.contains("pubfnupdate_widget_builder("));
    assert!(compact.contains("pubstructUpdateWidgetBuilder<'a>"));
    assert!(
        compact.contains("pubasyncfnsend(self)->Result<Widget,ApiOpError<UpdateWidgetApiError>>")
    );
    assert!(compact.contains("self.client.update_widget("));
    assert!(compact.contains("pubfnsearch(mutself,search:implInto<String>)->Self"));
    assert!(compact.contains("pubfnlimit(mutself,limit:i32)->Self"));
    assert!(compact.contains("pubfntags(mutself,tags:Vec<String>)->Self"));
    assert!(compact.contains("pubfnx_trace(mutself,x_trace:implInto<String>)->Self"));
    assert!(compact.contains("pubfninstructions(mutself,instructions:String)->Self"));
    assert!(compact.contains("pubfnwith_send(mutself,send:String)->Self"));
    assert!(compact.contains("pubfnwith_with_send(mutself,with_send:String)->Self"));
    assert!(compact.contains("pubfnwith_request(mutself,request:String)->Self"));
    assert!(compact.contains("pubfnconnection_string(mutself,connection_string:String)->Self"));
    assert!(compact.contains("pubfnconnection_string_2(mutself,connection_string_2:String)->Self"));
    assert!(compact.contains("pubfnpatch_profile_builder(&self)->PatchProfileBuilder<'_>"));
    assert!(compact.contains("pubfnsave_draft_builder(&self)->SaveDraftBuilder<'_>"));
    assert!(compact.contains("pubstructClientBuilder2<'a>"));
    assert!(compact.contains("pubfnclient_builder(&self,client_2:implInto<String>)"));
    assert!(compact.contains("pubfncreate_composed_builder("));
    assert!(compact.contains("self.request.alpha=Some(alpha)"));
}

#[test]
fn generated_flat_and_builder_calls_compile_together() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let mut analyzer = SchemaAnalyzer::new(builder_spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "operation_builders".into(),
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        builders: BuildersSection {
            enabled: true,
            threshold: 3,
        },
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

pub async fn both_calls_compile(client: &generated::HttpClient) {
    let flat_request = generated::UpdateWidgetRequest::new("flat".to_string());
    let _ = client.update_widget(
        "widget-1",
        None::<String>,
        None,
        None,
        None::<String>,
        "tenant-1",
        flat_request,
    ).await;

    let _ = client.update_widget_builder(
        "widget-2",
        "tenant-2",
        "builder".to_string(),
    )
    .search("needle")
    .limit(20)
    .tags(vec!["a".to_string(), "b".to_string()])
    .x_trace("trace-id")
    .instructions("be concise".to_string())
    .with_send("reserved".to_string())
    .with_with_send("collision".to_string())
    .with_request("field".to_string())
    .connection_string("camel".to_string())
    .connection_string_2("snake".to_string())
    .send()
    .await;

    let _ = client
        .patch_profile_builder()
        .title("Default-backed".to_string())
        .send()
        .await;

    let _ = client
        .save_draft_builder()
        .bio("Optional body initialized on demand".to_string())
        .send()
        .await;

    let _ = client
        .client_builder("path-client")
        .one("1")
        .two("2")
        .three("3")
        .four("4")
        .send()
        .await;
}

pub async fn composed_call_compiles(client: &generated::HttpClient) {
    let _ = client
        .create_composed_builder()
        .alpha("nested".to_string())
        .beta(true)
        .gamma(7)
        .delta("path".to_string())
        .send()
        .await;
}
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "operation-builder-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
reqwest = { version = "0.12", features = ["json", "multipart"] }
reqwest-middleware = { version = "0.4", features = ["multipart"] }
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/operation-builder-smoke"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "flat and builder calls failed to compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn openai_composition_body_gets_reachable_field_setters() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let spec: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("tests/fixtures/openai-responses.json"))
            .unwrap(),
    )
    .unwrap();
    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "openai_builder".into(),
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        builders: BuildersSection {
            enabled: true,
            threshold: 3,
        },
        client: Some(ClientSection {
            operations: vec!["createResponse".into()],
            prune_models: true,
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    let client = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap();
    assert!(client.content.contains("pub fn create_response_builder"));
    assert!(client.content.contains("pub fn instructions"));
    generator.write_files(&result).unwrap();

    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

pub async fn composed_body_builder_compiles(
    client: &generated::HttpClient,
    input: generated::CreateResponseInput,
    model: generated::ModelIdsResponses,
) {
    let _ = client
        .create_response_builder(input, model)
        .instructions("Answer briefly".to_string())
        .send()
        .await;
}
"#,
    )
    .unwrap();
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
name = "openai-operation-builder-smoke"
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
            manifest_dir.join("target/openai-operation-builder-smoke"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "OpenAI composition builder failed to compile:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
