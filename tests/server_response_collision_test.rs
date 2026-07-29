use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

#[test]
fn response_enums_avoid_retained_model_names_and_compile() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "response enum collisions", "version": "1.0.0" },
        "paths": {
            "/thing": { "get": {
                "operationId": "getThing",
                "tags": ["Things"],
                "responses": { "200": {
                    "description": "ok",
                    "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/GetThingResponse" }
                    }}
                }}
            }},
            "/things": { "get": {
                "operationId": "listThings",
                "tags": ["Things"],
                "responses": {
                    "200": {
                        "description": "ok",
                        "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/ListThingsResponse" }
                        }}
                    },
                    "400": {
                        "description": "bad request",
                        "content": { "application/json": {
                            "schema": { "$ref": "#/components/schemas/ListThingsServerResponse" }
                        }}
                    }
                }
            }}
        },
        "components": { "schemas": {
            "GetThingResponse": { "type": "string" },
            "ListThingsResponse": { "type": "string" },
            "ListThingsServerResponse": { "type": "string" }
        }}
    });
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let server = ServerSection {
        framework: "axum".into(),
        // Reverse lexical order to verify allocation is not selector-order dependent.
        operations: vec!["listThings".into(), "getThing".into()],
        prune_models: true,
        validation: ServerValidationSection {
            enabled: false,
            ..Default::default()
        },
    };
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "collision".into(),
        enable_async_client: false,
        server: Some(server),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    let types = std::fs::read_to_string(output_dir.join("types.rs")).unwrap();
    assert!(types.contains("pub type GetThingResponse = String;"));
    assert!(types.contains("pub type ListThingsResponse = String;"));
    assert!(types.contains("pub type ListThingsServerResponse = String;"));

    let errors = std::fs::read_to_string(output_dir.join("server/errors.rs")).unwrap();
    assert!(errors.contains("pub enum GetThingServerResponse"));
    assert!(errors.contains("Ok(GetThingResponse)"));
    assert!(errors.contains("pub enum ListThingsServerResponse2"));
    assert!(errors.contains("Ok(ListThingsResponse)"));
    assert!(errors.contains("BadRequest(ListThingsServerResponse)"));
    assert!(!errors.contains("pub enum GetThingResponse"));
    assert!(!errors.contains("pub enum ListThingsResponse"));
    assert!(!errors.contains("pub enum ListThingsServerResponse {"));

    let api = std::fs::read_to_string(output_dir.join("server/api.rs")).unwrap();
    assert!(api.contains("async fn get_thing(&self) -> GetThingServerResponse;"));
    assert!(api.contains("async fn list_things(&self) -> ListThingsServerResponse2;"));

    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

use generated::server::{
    GetThingServerResponse, ListThingsServerResponse2, ThingsApi,
};

#[derive(Clone)]
pub struct Api;

#[async_trait::async_trait]
impl ThingsApi for Api {
    async fn get_thing(&self) -> GetThingServerResponse {
        GetThingServerResponse::Ok("one".to_string())
    }

    async fn list_things(&self) -> ListThingsServerResponse2 {
        ListThingsServerResponse2::Ok("many".to_string())
    }
}
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "server-response-collision-smoke"
version = "0.1.0"
edition = "2024"

[dependencies]
async-trait = "0.1"
axum = "0.8"
serde = { version = "1", features = ["derive"] }
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated collision server failed to compile:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
