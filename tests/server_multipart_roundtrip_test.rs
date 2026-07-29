use openapi_to_rust::config::{ServerSection, ServerValidationSection};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

#[test]
fn generated_client_and_server_round_trip_typed_multipart() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "multipart roundtrip", "version": "1" },
        "paths": { "/upload": { "post": {
            "operationId": "uploadFile",
            "requestBody": { "required": true, "content": {
                "multipart/form-data": { "schema": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["file", "count", "enabled"],
                    "properties": {
                        "file": { "type": "string", "format": "binary" },
                        "count": { "type": "integer", "format": "uint64", "minimum": 1 },
                        "enabled": { "type": "boolean" },
                        "display-name": { "type": "string", "minLength": 2 }
                    }
                }}
            }},
            "responses": { "204": { "description": "accepted" } }
        }}}
    });
    let temp = tempfile::TempDir::new().expect("temp crate");
    let output_dir = temp.path().join("src/generated");
    let config = GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "multipart_roundtrip".into(),
        enable_async_client: true,
        tracing_enabled: false,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["uploadFile".into()],
            prune_models: true,
            validation: ServerValidationSection {
                enabled: true,
                max_body_bytes: 1024,
                max_errors: 4,
            },
        }),
        ..Default::default()
    };
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis).expect("generation");
    generator
        .write_files(&result)
        .expect("write generated files");

    let deps = std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml")).unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"generated-multipart-roundtrip\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n{deps}\n[dev-dependencies]\naxum = {{ version = \"0.8\", features = [\"tokio\", \"http1\", \"multipart\"] }}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"net\", \"sync\"] }}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::*;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

    #[derive(Clone)]
    struct Api {
        captured: UnboundedSender<(Vec<u8>, u64, bool, Option<String>)>,
    }

    #[async_trait::async_trait]
    impl ServerApi for Api {
        async fn upload_file(&self, body: UploadFileRequest) -> UploadFileResponse {
            self.captured.send((
                body.file.to_vec(),
                body.count,
                body.enabled,
                body.display_name,
            )).unwrap();
            UploadFileResponse::NoContent
        }
    }

    #[tokio::test]
    async fn multipart_contract() {
        let (tx, mut rx) = unbounded_channel();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, server_api_router(Api { captured: tx })).await.unwrap();
        });
        let base_url = format!("http://{address}");
        let client = HttpClient::new().with_base_url(base_url.clone());
        let payload = vec![0, 1, 2, 255];
        let request = UploadFileRequest {
            file: bytes::Bytes::from(payload.clone()),
            count: 9_223_372_036_854_775_808_u64,
            enabled: true,
            display_name: Some("demo".into()),
        };
        client.upload_file(request).await.unwrap();
        assert_eq!(
            rx.recv().await.unwrap(),
            (
                payload,
                9_223_372_036_854_775_808_u64,
                true,
                Some("demo".into())
            )
        );

        let invalid_typed = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(vec![1]))
            .text("count", "0")
            .text("enabled", "true")
            .text("display-name", "x");
        let response = reqwest::Client::new()
            .post(format!("{base_url}/upload"))
            .multipart(invalid_typed)
            .send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);

        let duplicate = reqwest::multipart::Form::new()
            .text("file", "a")
            .text("file", "b")
            .text("count", "7")
            .text("enabled", "true");
        let response = reqwest::Client::new()
            .post(format!("{base_url}/upload"))
            .multipart(duplicate)
            .send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);

        let constraint_violation = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(vec![1, 2, 3]))
            .text("count", "0")
            .text("enabled", "true")
            .text("display-name", "x");
        let response = reqwest::Client::new()
            .post(format!("http://{address}/upload"))
            .multipart(constraint_violation)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            reqwest::StatusCode::UNPROCESSABLE_ENTITY
        );

        let oversized = reqwest::multipart::Form::new()
            .part("file", reqwest::multipart::Part::bytes(vec![7; 2048]))
            .text("count", "7")
            .text("enabled", "true");
        let response = reqwest::Client::new()
            .post(format!("{base_url}/upload"))
            .multipart(oversized)
            .send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

        server.abort();
    }
}
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .args([
            "test",
            "--lib",
            "multipart_contract",
            "--",
            "--exact",
            "--nocapture",
        ])
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", "target/generated-multipart-roundtrip")
        .output()
        .expect("scratch cargo test");
    assert!(
        output.status.success(),
        "generated multipart roundtrip failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
