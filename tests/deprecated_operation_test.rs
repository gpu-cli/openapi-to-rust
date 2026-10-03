//! An operation the document marks `deprecated: true` generates client methods
//! marked `#[deprecated]`, so calling one warns, while the generated code that
//! calls them on the caller's behalf, a builder's `send`, doesn't warn itself.

use openapi_to_rust::config::BuildersSection;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn spec() -> serde_json::Value {
    // Four optional parameters, past the default builder threshold of three.
    let parameters: Vec<_> = ["a", "b", "c", "d"]
        .map(|name| json!({ "name": name, "in": "query", "schema": { "type": "string" } }))
        .into();
    json!({
        "openapi": "3.0.3",
        "info": { "title": "deprecated operations", "version": "1.0.0" },
        "paths": {
            "/old": { "get": {
                "operationId": "listOld",
                "deprecated": true,
                "parameters": parameters,
                "responses": { "204": { "description": "done" } }
            }},
            "/new": { "get": {
                "operationId": "listNew",
                "parameters": parameters,
                "responses": { "204": { "description": "done" } }
            }}
        }
    })
}

#[test]
fn deprecated_operations_warn_their_callers_and_not_the_generated_code() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut analyzer = SchemaAnalyzer::new(spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    let output_dir = temp.path().join("src/generated");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "deprecated_operations".into(),
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
    let client = &result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("client.rs"))
        .unwrap()
        .content;
    assert!(client.contains("#[deprecated]\n    pub async fn list_old("));
    assert!(client.contains("#[deprecated]\n    pub fn list_old_builder("));
    assert!(!client.contains("#[deprecated]\n    pub async fn list_new("));
    assert!(!client.contains("#[deprecated]\n    pub fn list_new_builder("));
    generator.write_files(&result).unwrap();

    // Deprecation is an error here, so the build fails exactly where the
    // deprecated operation is called, and only there.
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"#![deny(deprecated)]
pub mod generated;

pub async fn calls(client: &generated::HttpClient) {
    let _ = client.list_new(None::<&str>, None::<&str>, None::<&str>, None::<&str>).await;
    let _ = client.list_new_builder().a("x").send().await;
    let _ = client.list_old(None::<&str>, None::<&str>, None::<&str>, None::<&str>).await;
    let _ = client.list_old_builder().a("x").send().await;
}
"#,
    )
    .unwrap();
    let generated_dependencies =
        std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "deprecated-operation-smoke"
version = "0.1.0"
edition = "2024"

{generated_dependencies}
"#
        ),
    )
    .unwrap();

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .arg("--message-format=short")
        .arg("--color=never")
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            manifest_dir.join("target/deprecated-operation-smoke"),
        )
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    let errors: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("error: use of deprecated"))
        .collect();
    assert!(
        !output.status.success(),
        "calling a deprecated operation compiled:\n{stderr}"
    );
    assert_eq!(
        errors.len(),
        2,
        "expected exactly the two calls to `list_old` to be deprecated:\n{stderr}"
    );
    assert!(
        errors.iter().all(|line| line.starts_with("src/lib.rs:")),
        "the generated code uses a deprecated item itself:\n{stderr}"
    );
    assert!(errors.iter().any(|line| line.contains("list_old`")));
    assert!(errors.iter().any(|line| line.contains("list_old_builder`")));
}
