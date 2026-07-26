use openapi_to_rust::config::ServerSection;
use openapi_to_rust::server::codegen::ServerCodegen;
use openapi_to_rust::{GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

#[test]
fn server_generation_reports_colliding_raw_tags_and_rust_identifier() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "tag collisions", "version": "1.0.0" },
        "paths": {
            "/hyphen": { "get": {
                "operationId": "getHyphen",
                "tags": ["foo-bar"],
                "responses": { "204": { "description": "ok" } }
            }},
            "/underscore": { "get": {
                "operationId": "getUnderscore",
                "tags": ["foo_bar"],
                "responses": { "204": { "description": "ok" } }
            }}
        }
    });
    let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let server = ServerSection {
        framework: "axum".into(),
        // Reverse lexical order to verify the diagnostic remains stable.
        operations: vec!["getUnderscore".into(), "getHyphen".into()],
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
        .unwrap_err()
        .to_string();
    assert!(error.contains("foo-bar"), "{error}");
    assert!(error.contains("foo_bar"), "{error}");
    assert!(error.contains("FooBarApi"), "{error}");
    assert!(error.contains("Rename one tag"), "{error}");
}
