use openapi_to_rust::config::{ClientSection, ServerSection, ServerValidationSection};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn raw_body_spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "raw request bodies", "version": "1.0.0" },
        "paths": {
            "/octets": { "post": {
                "operationId": "uploadOctets", "tags": ["Binary"],
                "requestBody": { "required": true, "content": {
                    "application/octet-stream; profile=v2": { "schema": { "type": "string", "format": "binary" } }
                }},
                "responses": { "204": { "description": "ok" } }
            }},
            "/archive": { "post": {
                "operationId": "uploadArchive", "tags": ["Binary"],
                "requestBody": { "required": true, "content": {
                    "application/vnd.acme+zip; profile=v2": { "schema": { "type": "string", "format": "binary" } }
                }},
                "responses": { "204": { "description": "ok" } }
            }},
            "/optional-octets": { "post": {
                "operationId": "maybeOctets", "tags": ["Binary"],
                "requestBody": { "content": {
                    "application/octet-stream": { "schema": { "type": "string", "format": "binary" } }
                }},
                "responses": { "204": { "description": "ok" } }
            }},
            "/beacon": { "post": {
                "operationId": "sendBeacon", "tags": ["Text"],
                "requestBody": { "required": true, "content": {
                    "text/plain; charset=utf-8": { "schema": { "type": "string" } }
                }},
                "responses": { "204": { "description": "ok" } }
            }},
            "/optional-beacon": { "post": {
                "operationId": "maybeBeacon", "tags": ["Text"],
                "requestBody": { "content": {
                    "text/plain": { "schema": { "type": "string" } }
                }},
                "responses": { "204": { "description": "ok" } }
            }}
        }
    })
}

#[test]
fn raw_transport_decoders_coexist_with_schema_validation() {
    let mut analysis = SchemaAnalyzer::new(raw_body_spec())
        .unwrap()
        .analyze()
        .unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: false,
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: vec!["tag:Binary".into(), "tag:Text".into()],
            prune_models: true,
            validation: Default::default(),
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    let validation = result
        .files
        .iter()
        .find(|file| file.path.ends_with("server/validation.rs"))
        .unwrap();
    assert!(validation.content.contains("decode_binary_body"));
    assert!(validation.content.contains("decode_text_body"));
    assert!(validation.content.contains("jsonschema"));
}

fn generated_client_and_server_round_trip_bounded_raw_bodies(validation_enabled: bool) {
    let temp = tempfile::TempDir::new().expect("temp crate");
    let output_dir = temp.path().join("src/generated");
    let server = ServerSection {
        framework: "axum".into(),
        operations: vec!["tag:Binary".into(), "tag:Text".into()],
        prune_models: true,
        validation: ServerValidationSection {
            enabled: validation_enabled,
            max_body_bytes: 8,
            max_errors: 2,
        },
    };
    let config = GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "raw_body".into(),
        enable_async_client: true,
        tracing_enabled: false,
        server: Some(server),
        ..Default::default()
    };
    let mut analysis = SchemaAnalyzer::new(raw_body_spec())
        .expect("analyzer")
        .analyze()
        .expect("analysis");
    assert!(matches!(
        analysis.operations["uploadArchive"].request_body,
        Some(openapi_to_rust::analysis::RequestBodyContent::Binary { ref media_type })
            if media_type == "application/vnd.acme+zip; profile=v2"
    ));
    assert!(matches!(
        analysis.operations["uploadOctets"].request_body,
        Some(openapi_to_rust::analysis::RequestBodyContent::OctetStream { ref media_type })
            if media_type == "application/octet-stream; profile=v2"
    ));
    assert!(matches!(
        analysis.operations["sendBeacon"].request_body,
        Some(openapi_to_rust::analysis::RequestBodyContent::TextPlain { ref media_type })
            if media_type == "text/plain; charset=utf-8"
    ));
    let generator = CodeGenerator::new(config);
    let result = generator
        .generate_all(&mut analysis)
        .expect("client and server generate");
    generator
        .write_files(&result)
        .expect("generated files write");

    let dependency_fragment =
        std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml")).unwrap();
    for dependency in ["bytes", "http-body-util", "mime", "axum", "reqwest"] {
        assert!(
            dependency_fragment.contains(dependency),
            "{dependency_fragment}"
        );
    }
    assert_eq!(
        dependency_fragment.contains("jsonschema"),
        validation_enabled,
        "{dependency_fragment}"
    );

    let package = r#"[package]
name = "generated-raw-body-roundtrip"
version = "0.0.0"
edition = "2024"
publish = false
"#;
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "{package}\n{dependency_fragment}\n[dev-dependencies]\naxum = {{ version = \"0.8\", features = [\"tokio\", \"http1\"] }}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"net\", \"sync\", \"time\"] }}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::*;
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    #[derive(Clone)]
    struct Api {
        captured: UnboundedSender<(String, Option<Vec<u8>>)>,
    }

    #[async_trait::async_trait]
    impl BinaryApi for Api {
        async fn upload_octets(&self, body: bytes::Bytes) -> UploadOctetsResponse {
            self.captured.send(("octets".into(), Some(body.to_vec()))).unwrap();
            UploadOctetsResponse::NoContent
        }

        async fn upload_archive(&self, body: bytes::Bytes) -> UploadArchiveResponse {
            self.captured.send(("archive".into(), Some(body.to_vec()))).unwrap();
            UploadArchiveResponse::NoContent
        }

        async fn maybe_octets(&self, body: Option<bytes::Bytes>) -> MaybeOctetsResponse {
            self.captured.send(("maybe-octets".into(), body.map(|value| value.to_vec()))).unwrap();
            MaybeOctetsResponse::NoContent
        }
    }

    #[async_trait::async_trait]
    impl TextApi for Api {
        async fn send_beacon(&self, body: String) -> SendBeaconResponse {
            self.captured.send(("beacon".into(), Some(body.into_bytes()))).unwrap();
            SendBeaconResponse::NoContent
        }

        async fn maybe_beacon(&self, body: Option<String>) -> MaybeBeaconResponse {
            self.captured.send(("maybe-beacon".into(), body.map(String::into_bytes))).unwrap();
            MaybeBeaconResponse::NoContent
        }
    }

    #[tokio::test]
    async fn raw_transport_contract() {
        let (captured_tx, mut captured_rx) = unbounded_channel();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let binary_api = Api { captured: captured_tx.clone() };
        let text_api = Api { captured: captured_tx };
        let server = tokio::spawn(async move {
            axum::serve(listener, build_router(binary_api, text_api)).await.unwrap();
        });
        let base_url = format!("http://{address}");
        let client = HttpClient::new().with_base_url(base_url.clone());

        let octets = vec![0, 159, 146, 150, 255];
        client.upload_octets(octets.clone()).await.unwrap();
        assert_eq!(captured_rx.recv().await.unwrap(), ("octets".into(), Some(octets)));

        let archive = vec![0x50, 0x4b, 0xff];
        client.upload_archive(archive.clone()).await.unwrap();
        assert_eq!(captured_rx.recv().await.unwrap(), ("archive".into(), Some(archive)));

        client.send_beacon("signal".to_string()).await.unwrap();
        assert_eq!(captured_rx.recv().await.unwrap(), ("beacon".into(), Some(b"signal".to_vec())));

        client.maybe_octets(None).await.unwrap();
        assert_eq!(captured_rx.recv().await.unwrap(), ("maybe-octets".into(), None));
        client.maybe_beacon(None).await.unwrap();
        assert_eq!(captured_rx.recv().await.unwrap(), ("maybe-beacon".into(), None));

        let http = reqwest::Client::new();
        let wrong = http.post(format!("{base_url}/octets"))
            .header("content-type", "text/plain").body(vec![1]).send().await.unwrap();
        assert_eq!(wrong.status(), reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let wrong_suffix = http.post(format!("{base_url}/archive"))
            .header("content-type", "application/vnd.acme+json; profile=v2")
            .body(vec![1]).send().await.unwrap();
        assert_eq!(wrong_suffix.status(), reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let missing_profile = http.post(format!("{base_url}/archive"))
            .header("content-type", "application/vnd.acme+zip")
            .body(vec![1]).send().await.unwrap();
        assert_eq!(missing_profile.status(), reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let missing = http.post(format!("{base_url}/octets"))
            .body(vec![1]).send().await.unwrap();
        assert_eq!(missing.status(), reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let oversized = http.post(format!("{base_url}/octets"))
            .header("content-type", "application/octet-stream; profile=v2")
            .body(vec![7; 9]).send().await.unwrap();
        assert_eq!(oversized.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

        let invalid_utf8 = http.post(format!("{base_url}/beacon"))
            .header("content-type", "text/plain; charset=utf-8")
            .body(vec![0xff]).send().await.unwrap();
        assert_eq!(invalid_utf8.status(), reqwest::StatusCode::BAD_REQUEST);

        let optional_nonempty_without_media = http.post(format!("{base_url}/optional-beacon"))
            .body("x").send().await.unwrap();
        assert_eq!(optional_nonempty_without_media.status(), reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let required_empty = http.post(format!("{base_url}/octets"))
            .header("content-type", "application/octet-stream; profile=v2; version=1")
            .body(Vec::new()).send().await.unwrap();
        assert_eq!(required_empty.status(), reqwest::StatusCode::NO_CONTENT);
        assert_eq!(captured_rx.recv().await.unwrap(), ("octets".into(), Some(Vec::new())));

        server.abort();
    }
}
"#,
    )
    .unwrap();

    let mode = if validation_enabled {
        "enabled"
    } else {
        "disabled"
    };
    let scratch_target = format!("/private/tmp/raw-body-target/{mode}");
    let scratch_build = format!("/private/tmp/raw-body-build/{mode}");
    let check = Command::new("cargo")
        .args(["check", "--lib", "--offline"])
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", &scratch_target)
        .env("CARGO_BUILD_BUILD_DIR", &scratch_build)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "dependency-fragment compile failed:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    let test = Command::new("cargo")
        .args(["test", "--offline", "--quiet"])
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", &scratch_target)
        .env("CARGO_BUILD_BUILD_DIR", &scratch_build)
        .output()
        .unwrap();
    assert!(
        test.status.success(),
        "generated client/server runtime failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
}

#[test]
fn generated_client_and_server_round_trip_bounded_raw_bodies_with_validation() {
    generated_client_and_server_round_trip_bounded_raw_bodies(true);
}

#[test]
fn generated_client_and_server_round_trip_bounded_raw_bodies_without_validation() {
    generated_client_and_server_round_trip_bounded_raw_bodies(false);
}

#[test]
fn storyden_generated_client_round_trips_raw_bodies_through_generated_server() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source = std::fs::read_to_string(manifest_dir.join("specs/storyden.yaml"))
        .expect("read vendored Storyden spec");
    let spec = serde_yaml::from_str(&source).expect("parse vendored Storyden spec");
    let selected = vec![
        "IconUpload".to_string(),
        "PluginUpdatePackage".to_string(),
        "SendBeacon".to_string(),
    ];
    let temp = tempfile::TempDir::new().expect("temp crate");
    let output_dir = temp.path().join("src/generated");
    let config = GeneratorConfig {
        output_dir: output_dir.clone(),
        module_name: "storyden".into(),
        enable_async_client: true,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: selected.clone(),
            prune_models: true,
        }),
        server: Some(ServerSection {
            framework: "axum".into(),
            operations: selected,
            prune_models: true,
            validation: ServerValidationSection {
                enabled: false,
                max_body_bytes: 32,
                max_errors: 2,
            },
        }),
        ..Default::default()
    };
    let mut analysis = SchemaAnalyzer::new(spec)
        .expect("analyzer")
        .analyze()
        .expect("analysis");
    let generator = CodeGenerator::new(config);
    let result = generator
        .generate_all(&mut analysis)
        .expect("selected Storyden client and server generate");
    generator
        .write_files(&result)
        .expect("generated Storyden files write");

    let dependency_fragment =
        std::fs::read_to_string(output_dir.join("REQUIRED_DEPS.toml")).unwrap();
    let package = r#"[package]
name = "generated-storyden-roundtrip"
version = "0.0.0"
edition = "2024"
publish = false
"#;
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "{package}\n{dependency_fragment}\n[dev-dependencies]\naxum = {{ version = \"0.8\", features = [\"tokio\", \"http1\"] }}\ntokio = {{ version = \"1\", features = [\"macros\", \"rt-multi-thread\", \"net\", \"sync\"] }}\n"
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::*;
    use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

    #[derive(Clone)]
    struct Api {
        captured: UnboundedSender<(String, Option<Vec<u8>>)>,
    }

    #[async_trait::async_trait]
    impl MiscApi for Api {
        async fn icon_upload(&self, body: Option<bytes::Bytes>) -> IconUploadResponse {
            self.captured
                .send(("icon".into(), body.map(|value| value.to_vec())))
                .unwrap();
            IconUploadResponse::Ok
        }

        async fn send_beacon(&self, body: Option<String>) -> SendBeaconResponse {
            self.captured
                .send(("beacon".into(), body.map(String::into_bytes)))
                .unwrap();
            SendBeaconResponse::Accepted
        }
    }

    #[async_trait::async_trait]
    impl PluginsApi for Api {
        async fn plugin_update_package(
            &self,
            plugin_instance_id: String,
            body: Option<bytes::Bytes>,
        ) -> PluginUpdatePackageResponse {
            assert_eq!(plugin_instance_id, "plugin-one");
            self.captured
                .send(("plugin".into(), body.map(|value| value.to_vec())))
                .unwrap();
            PluginUpdatePackageResponse::Ok(serde_json::json!({"updated": true}))
        }
    }

    #[tokio::test]
    async fn selected_storyden_contract() {
        let (captured_tx, mut captured_rx) = unbounded_channel();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let misc_api = Api { captured: captured_tx.clone() };
        let plugins_api = Api { captured: captured_tx };
        let server = tokio::spawn(async move {
            axum::serve(listener, build_router(misc_api, plugins_api)).await.unwrap();
        });
        let client = HttpClient::new().with_base_url(format!("http://{address}"));

        let icon = vec![0, 159, 146, 150, 255];
        client.icon_upload(Some(icon.clone())).await.unwrap();
        assert_eq!(captured_rx.recv().await.unwrap(), ("icon".into(), Some(icon)));

        let archive = vec![0x50, 0x4b, 0x03, 0x04, 0xff];
        let response = client
            .plugin_update_package("plugin-one", Some(archive.clone()))
            .await
            .unwrap();
        assert_eq!(response, serde_json::json!({"updated": true}));
        assert_eq!(
            captured_rx.recv().await.unwrap(),
            ("plugin".into(), Some(archive))
        );

        client.send_beacon(Some("read-state".into())).await.unwrap();
        assert_eq!(
            captured_rx.recv().await.unwrap(),
            ("beacon".into(), Some(b"read-state".to_vec()))
        );

        server.abort();
    }
}
"#,
    )
    .unwrap();

    let test = Command::new("cargo")
        .args(["test", "--offline", "--quiet"])
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", "/private/tmp/storyden-roundtrip-target")
        .env(
            "CARGO_BUILD_BUILD_DIR",
            "/private/tmp/storyden-roundtrip-build",
        )
        .output()
        .unwrap();
    assert!(
        test.status.success(),
        "generated Storyden client/server runtime failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
}
