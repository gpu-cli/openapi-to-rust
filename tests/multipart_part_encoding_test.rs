//! Multipart fields follow OpenAPI's encoding defaults. An object field, or a
//! union of them, is one JSON part, typed `application/json` unless
//! `encoding.<field>.contentType` names another type. A single media type in
//! `encoding.<field>.contentType` is the type of any part, and a list of them
//! is the set the server accepts, so the part gets none.
//!
//! The generated client sends a form to a local server, which records each
//! part's name, content type and body.

use openapi_to_rust::config::ClientSection;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn spec() -> serde_json::Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "multipart encoding", "version": "1" },
        "components": { "schemas": {
            "Plain": { "type": "object", "properties": { "plain": { "type": "string" } } },
            "Styled": { "type": "object", "properties": { "styled": { "type": "boolean" } } }
        }},
        "paths": {
            "/scripts": { "put": {
                "operationId": "uploadScript",
                "requestBody": { "required": true, "content": { "multipart/form-data": {
                    "schema": {
                        "type": "object",
                        "required": ["metadata"],
                        "properties": {
                            "metadata": {
                                "type": "object",
                                "properties": { "main_module": { "type": "string" } }
                            },
                            "settings": { "type": "object", "additionalProperties": true },
                            "theme": { "oneOf": [
                                { "$ref": "#/components/schemas/Plain" },
                                { "$ref": "#/components/schemas/Styled" }
                            ]},
                            "logo": { "type": "string", "format": "binary" },
                            "module": { "type": "string", "format": "binary" },
                            "caption": { "type": "string" },
                            "count": { "type": "integer" }
                        }
                    },
                    "encoding": {
                        "metadata": { "contentType": "application/vnd.script+json" },
                        "logo": { "contentType": "image/png" },
                        "module": { "contentType": "application/javascript+module, text/javascript" },
                        "caption": { "contentType": "text/plain; charset=utf-8" },
                        "count": { "contentType": "text/*" }
                    }
                }}},
                "responses": { "204": { "description": "ok" } }
            }}
        }
    })
}

#[test]
fn multipart_parts_follow_openapi_encoding() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut analyzer = SchemaAnalyzer::new(spec()).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: temp.path().join("src/generated"),
        enable_async_client: true,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec!["uploadScript".into()],
            prune_models: true,
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();
    let client = result
        .files
        .iter()
        .find(|file| file.path.ends_with("client.rs"))
        .unwrap();
    assert!(
        !client
            .content
            .contains("must be binary or a scalar text field"),
        "object fields are sent, not rejected:\n{}",
        client.content
    );

    let dependencies =
        std::fs::read_to_string(temp.path().join("src/generated/REQUIRED_DEPS.toml")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "generated-multipart-encoding"
version = "0.0.0"
edition = "2024"
publish = false
{dependencies}
[dev-dependencies]
axum = {{version="0.8", features=["multipart"]}}
tokio = {{version="1", features=["macros","net","rt-multi-thread"]}}
"#
        ),
    )
    .unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), RUNTIME).unwrap();

    let target_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/generated-multipart-encoding");
    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated multipart client failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    if Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line == "wasm32-unknown-unknown")
        })
    {
        let output = Command::new("cargo")
            .args([
                "check",
                "--quiet",
                "--lib",
                "--target",
                "wasm32-unknown-unknown",
            ])
            .current_dir(temp.path())
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "generated multipart client failed to compile for wasm32:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

const RUNTIME: &str = r###"pub mod generated;
#[cfg(test)]
mod tests {
    use super::generated::{client::HttpClient, types::*};
    use axum::{extract::{Multipart, State}, http::StatusCode, routing::put, Router};
    use std::{collections::BTreeMap, sync::{Arc, Mutex}};

    /// Each part's content type and body, by name.
    type Parts = BTreeMap<String, (Option<String>, Vec<u8>)>;

    async fn record(State(parts): State<Arc<Mutex<Parts>>>, mut multipart: Multipart) -> StatusCode {
        while let Some(field) = multipart.next_field().await.unwrap() {
            let name = field.name().unwrap().to_string();
            let content_type = field.content_type().map(str::to_string);
            let body = field.bytes().await.unwrap().to_vec();
            parts.lock().unwrap().insert(name, (content_type, body));
        }
        StatusCode::NO_CONTENT
    }

    #[tokio::test]
    async fn parts_carry_their_encoding() {
        let parts = Arc::new(Mutex::new(Parts::new()));
        let app = Router::new().route("/scripts", put(record)).with_state(parts.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let mut settings = BTreeMap::new();
        settings.insert("compatibility_date".to_string(), serde_json::json!("2026-10-01"));
        let request = UploadScriptRequest {
            metadata: UploadScriptRequestMetadata { main_module: Some("worker.js".into()) },
            settings: Some(UploadScriptRequestSettings { additional_properties: settings }),
            theme: Some(UploadScriptRequestTheme::Styled(Styled { styled: Some(true) })),
            logo: Some(bytes::Bytes::from_static(b"\x89PNG")),
            module: Some(bytes::Bytes::from_static(b"export default {}")),
            caption: Some("hello".into()),
            count: Some(3),
        };
        HttpClient::new().with_base_url(base_url).upload_script(request).await.unwrap();

        let parts = parts.lock().unwrap().clone();
        let part = |name: &str| parts.get(name).unwrap_or_else(|| panic!("no part {name}: {parts:?}"));
        let json = |name: &str| serde_json::from_slice::<serde_json::Value>(&part(name).1).unwrap();

        // Objects are JSON, typed by `encoding` or `application/json`.
        assert_eq!(part("metadata").0.as_deref(), Some("application/vnd.script+json"));
        assert_eq!(json("metadata"), serde_json::json!({ "main_module": "worker.js" }));
        assert_eq!(part("settings").0.as_deref(), Some("application/json"));
        assert_eq!(json("settings"), serde_json::json!({ "compatibility_date": "2026-10-01" }));
        assert_eq!(part("theme").0.as_deref(), Some("application/json"));
        assert_eq!(json("theme"), serde_json::json!({ "styled": true }));

        // A single media type applies to any part.
        assert_eq!(part("logo"), &(Some("image/png".to_string()), b"\x89PNG".to_vec()));
        assert_eq!(part("caption"), &(Some("text/plain; charset=utf-8".to_string()), b"hello".to_vec()));

        // A list or a range is what the server accepts, not the part's type.
        assert_eq!(part("module"), &(None, b"export default {}".to_vec()));
        assert_eq!(part("count"), &(None, b"3".to_vec()));
    }
}
"###;
