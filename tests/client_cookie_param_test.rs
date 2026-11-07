//! String cookie parameters are `impl AsRef<str>` in generated clients and
//! must be formatted through `.as_ref()` (they are not `Display`).

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "cookie params", "version": "1.0.0" },
        "paths": { "/me": { "get": {
            "operationId": "me",
            "parameters": [
                { "name": "session", "in": "cookie", "required": true, "schema": { "type": "string" } },
                { "name": "theme", "in": "cookie", "schema": { "type": "string" } },
                { "name": "visits", "in": "cookie", "schema": { "type": "integer" } }
            ],
            "responses": { "200": { "description": "echo", "content": {
                "application/json": { "schema": { "type": "string" } }
            }}}
        }}}
    })
}

#[test]
fn string_cookie_parameters_compile_and_reach_the_wire() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: temp.path().join("src/generated"),
        module_name: "cookies".into(),
        enable_async_client: true,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();
    let dependencies =
        std::fs::read_to_string(temp.path().join("src/generated/REQUIRED_DEPS.toml")).unwrap();

    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "generated-client-cookies"
version = "0.0.0"
edition = "2024"
publish = false
{dependencies}
[dev-dependencies]
axum = "0.8"
tokio = {{version="1", features=["macros","net","rt-multi-thread"]}}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated::client::HttpClient;
    use axum::{http::HeaderMap, routing::get, Json, Router};

    #[tokio::test]
    async fn cookies_are_sent() {
        let app = Router::new().route("/me", get(|headers: HeaderMap| async move {
            Json(headers["cookie"].to_str().unwrap().to_owned())
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = HttpClient::new().with_base_url(format!("http://{address}"));

        let sent = client.me(String::from("abc"), Some("dark"), Some(3)).await.unwrap();
        assert_eq!(sent, "session=abc; theme=dark; visits=3");
        let sent = client.me("abc", None::<&str>, None).await.unwrap();
        assert_eq!(sent, "session=abc");
    }
}
"#,
    )
    .unwrap();

    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env("CARGO_TARGET_DIR", temp.path().join("cargo-target"))
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated client failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
