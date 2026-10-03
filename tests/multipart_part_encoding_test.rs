//! Multipart fields follow OpenAPI's encoding defaults. An object field, or a
//! union of them, is one JSON part, typed `application/json` unless
//! `encoding.<field>.contentType` names another type. A single media type in
//! `encoding.<field>.contentType` is the type of any part, and a list of them
//! is the set the server accepts, so the part gets none.
//!
//! A file field is a `MultipartFile`, which carries its own filename and
//! content type; its own content type wins over the declared one. A list is
//! one part per item under the field's name, which is how Cloudflare takes a
//! Worker's modules. A union of text and bytes is a file too, so it can send
//! either.
//!
//! The generated client sends forms to a local server, which records each
//! part's name, filename, content type and body. The client is generated
//! under each `types.binary` strategy, since `MultipartFile` holds its content
//! in that carrier.

use openapi_to_rust::config::ClientSection;
use openapi_to_rust::type_mapping::{BinaryStrategy, TypeMapper, TypeMappingConfig};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn spec() -> serde_json::Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "multipart encoding", "version": "1" },
        "components": { "schemas": {
            "Plain": { "type": "object", "properties": { "plain": { "type": "string" } } },
            "Styled": { "type": "object", "properties": { "styled": { "type": "boolean" } } },
            "Value": { "anyOf": [{ "type": "string" }, { "type": "string", "format": "binary" }] }
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
                            "files": { "type": "array", "items": { "type": "string", "format": "binary" } },
                            "logo": { "type": "string", "format": "binary" },
                            "icon": { "type": "string", "format": "binary" },
                            "caption": { "type": "string" },
                            "count": { "type": "integer" },
                            "tags": { "type": "array", "items": { "type": "string" } }
                        }
                    },
                    "encoding": {
                        "metadata": { "contentType": "application/vnd.script+json" },
                        "files": { "contentType": "application/javascript+module, text/javascript" },
                        "logo": { "contentType": "image/png" },
                        "icon": { "contentType": "image/png" },
                        "caption": { "contentType": "text/plain; charset=utf-8" },
                        "count": { "contentType": "text/*" }
                    }
                }}},
                "responses": { "204": { "description": "ok" } }
            }},
            "/values": { "put": {
                "operationId": "writeValue",
                "requestBody": { "required": true, "content": { "multipart/form-data": {
                    "schema": {
                        "type": "object",
                        "required": ["value"],
                        "properties": { "value": { "$ref": "#/components/schemas/Value" } }
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
    let mut dependencies = String::new();
    for (module, binary) in [
        ("with_bytes", BinaryStrategy::Bytes),
        ("with_vec", BinaryStrategy::VecU8),
        ("with_string", BinaryStrategy::String),
    ] {
        let types = TypeMappingConfig {
            binary,
            ..TypeMappingConfig::default()
        };
        let mut analyzer =
            SchemaAnalyzer::with_type_mapper(spec(), TypeMapper::new(types.clone())).unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = CodeGenerator::new(GeneratorConfig {
            output_dir: temp.path().join("src").join(module),
            types,
            enable_async_client: true,
            tracing_enabled: false,
            client: Some(ClientSection {
                operations: vec!["uploadScript".into(), "writeValue".into()],
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
                .contains("HttpError::Config(\"multipart field"),
            "every field is sent, none rejected:\n{}",
            client.content
        );
        if module == "with_bytes" {
            dependencies =
                std::fs::read_to_string(temp.path().join("src/with_bytes/REQUIRED_DEPS.toml"))
                    .unwrap();
        }
    }

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

const RUNTIME: &str = r###"pub mod with_bytes;
pub mod with_vec;
pub mod with_string;
#[cfg(test)]
mod tests {
    use axum::{extract::{Multipart, State}, http::StatusCode, routing::put, Router};
    use std::{collections::BTreeMap, sync::{Arc, Mutex}};

    /// A recorded part: its filename, content type and body.
    #[derive(Debug, Clone, PartialEq)]
    struct Part { file_name: Option<String>, content_type: Option<String>, body: Vec<u8> }

    impl Part {
        fn new(file_name: Option<&str>, content_type: Option<&str>, body: &[u8]) -> Self {
            Self { file_name: file_name.map(str::to_string), content_type: content_type.map(str::to_string), body: body.to_vec() }
        }
    }

    /// Every part of the last form, in order, by name.
    type Parts = Arc<Mutex<Vec<(String, Part)>>>;

    async fn record(State(parts): State<Parts>, mut multipart: Multipart) -> StatusCode {
        let mut form = Vec::new();
        while let Some(field) = multipart.next_field().await.unwrap() {
            let name = field.name().unwrap().to_string();
            let file_name = field.file_name().map(str::to_string);
            let content_type = field.content_type().map(str::to_string);
            let body = field.bytes().await.unwrap().to_vec();
            form.push((name, Part { file_name, content_type, body }));
        }
        *parts.lock().unwrap() = form;
        StatusCode::NO_CONTENT
    }

    async fn serve() -> (String, Parts) {
        let parts = Parts::default();
        let app = Router::new()
            .route("/scripts", put(record))
            .route("/values", put(record))
            .with_state(parts.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base_url, parts)
    }

    fn named<'a>(parts: &'a [(String, Part)], name: &str) -> Vec<&'a Part> {
        parts.iter().filter(|(part, _)| part == name).map(|(_, part)| part).collect()
    }

    /// The same form, whatever carries a file's content.
    macro_rules! upload_script_parts {
        ($module:ident) => {{
            use super::$module::{client::HttpClient, types::*};
            let (base_url, parts) = serve().await;
            let client = HttpClient::new().with_base_url(base_url);
            let mut settings = BTreeMap::new();
            settings.insert("compatibility_date".to_string(), serde_json::json!("2026-10-01"));
            let request = UploadScriptRequest {
                metadata: UploadScriptRequestMetadata { main_module: Some("worker.js".into()) },
                settings: Some(UploadScriptRequestSettings { additional_properties: settings }),
                theme: Some(UploadScriptRequestTheme::Styled(Styled { styled: Some(true) })),
                files: Some(vec![
                    MultipartFile::from("export default {}")
                        .with_file_name("worker.js")
                        .with_content_type("application/javascript+module"),
                    MultipartFile::from("export const x = 1").with_file_name("lib.js"),
                ]),
                logo: Some(MultipartFile::from("PNG")),
                icon: Some(MultipartFile::from("<svg/>").with_content_type("image/svg+xml")),
                caption: Some("hello".into()),
                count: Some(3),
                tags: Some(vec!["a".into(), "b".into()]),
            };
            client.upload_script(request).await.unwrap();
            let script = parts.lock().unwrap().clone();

            client.write_value(WriteValueRequest { value: "text".into() }).await.unwrap();
            let text = parts.lock().unwrap().clone();
            client
                .write_value(WriteValueRequest { value: MultipartFile::from("raw").with_file_name("value.bin") })
                .await
                .unwrap();
            let file = parts.lock().unwrap().clone();
            (script, text, file)
        }};
    }

    fn check(script: Vec<(String, Part)>, text: Vec<(String, Part)>, file: Vec<(String, Part)>) {
        let one = |name: &str| {
            let parts = named(&script, name);
            assert_eq!(parts.len(), 1, "{name}: {script:?}");
            parts[0].clone()
        };
        let json = |name: &str| serde_json::from_slice::<serde_json::Value>(&one(name).body).unwrap();

        // Objects are JSON, typed by `encoding` or `application/json`.
        assert_eq!(one("metadata").content_type.as_deref(), Some("application/vnd.script+json"));
        assert_eq!(json("metadata"), serde_json::json!({ "main_module": "worker.js" }));
        assert_eq!(one("settings").content_type.as_deref(), Some("application/json"));
        assert_eq!(json("settings"), serde_json::json!({ "compatibility_date": "2026-10-01" }));
        assert_eq!(one("theme").content_type.as_deref(), Some("application/json"));
        assert_eq!(json("theme"), serde_json::json!({ "styled": true }));

        // A list of files is one part each, with its own filename and
        // content type. The declared list isn't a type for the part.
        assert_eq!(named(&script, "files"), [
            &Part::new(Some("worker.js"), Some("application/javascript+module"), b"export default {}"),
            &Part::new(Some("lib.js"), None, b"export const x = 1"),
        ]);

        // A declared media type applies when the file has none, and the
        // file's own wins.
        assert_eq!(one("logo"), Part::new(None, Some("image/png"), b"PNG"));
        assert_eq!(one("icon"), Part::new(None, Some("image/svg+xml"), b"<svg/>"));
        assert_eq!(one("caption"), Part::new(None, Some("text/plain; charset=utf-8"), b"hello"));

        // A range is what the server accepts, not the part's type.
        assert_eq!(one("count"), Part::new(None, None, b"3"));

        // A list of scalars is one text part each.
        assert_eq!(named(&script, "tags"), [&Part::new(None, None, b"a"), &Part::new(None, None, b"b")]);

        // Text or bytes: a plain value, or a file with a filename.
        assert_eq!(named(&text, "value"), [&Part::new(None, None, b"text")]);
        assert_eq!(named(&file, "value"), [&Part::new(Some("value.bin"), None, b"raw")]);
    }

    #[tokio::test]
    async fn bytes_files() {
        let (script, text, file) = upload_script_parts!(with_bytes);
        check(script, text, file);
    }

    #[tokio::test]
    async fn vec_files() {
        let (script, text, file) = upload_script_parts!(with_vec);
        check(script, text, file);
    }

    #[tokio::test]
    async fn string_files() {
        let (script, text, file) = upload_script_parts!(with_string);
        check(script, text, file);
    }
}
"###;
