//! Cross-feature contract: preprocessing, client planning, metadata, and output
//! verification must all consume the same effective API document.

use serde_json::{Value, json};
use std::path::Path;
use std::process::{Command, Output};

fn generate(cwd: &Path, config: &Path, extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openapi-to-rust"))
        .current_dir(cwd)
        .arg("generate")
        .arg("--config")
        .arg(config)
        .args(extra)
        .output()
        .unwrap()
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn overlay_effective_document_bindings_and_client_stay_in_sync() {
    let temp = tempfile::tempdir().unwrap();
    let config_dir = temp.path().join("configuration");
    let cwd = temp.path().join("unrelated-working-directory");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "Combined feature pipeline", "version": "1.0" },
        "servers": [{ "url": "https://source.example.test" }],
        "paths": {
            "/legacy": { "get": {
                "operationId": "legacy",
                "responses": { "204": { "description": "obsolete" } }
            }},
            "/render": { "post": {
                "operationId": "render",
                "responses": { "200": { "description": "rendered", "content": {
                    "application/json": { "schema": {
                        "type": "object", "properties": { "status": { "type": "string" } }
                    } }
                } } }
            } }
        },
        "components": { "schemas": { "Unused": {
            "type": "object", "properties": { "value": { "type": "string" } }
        } } }
    });
    std::fs::write(config_dir.join("source.json"), spec.to_string()).unwrap();
    let overlay = json!({
        "overlay": "1.1.0",
        "info": { "title": "Effective API", "version": "1.0" },
        "actions": [
            { "target": "$.paths['/legacy']", "remove": true },
            { "target": "$.servers[0].url", "update": "https://effective.example.test" },
            { "target": "$.paths['/render'].post.responses['200'].content", "update": {
                "application/vnd.report+json": { "schema": {
                    "type": "object", "required": ["alternate"],
                    "properties": { "alternate": { "type": "integer" } }
                } },
                "application/octet-stream": { "schema": {
                    "type": "string", "format": "binary"
                } }
            } }
        ]
    });
    std::fs::write(config_dir.join("changes.overlay.json"), overlay.to_string()).unwrap();
    let config = config_dir.join("openapi-to-rust.toml");
    std::fs::write(
        &config,
        r#"
[generator]
spec_path = "source.json"
output_dir = "generated"
module_name = "combined"
overlays = ["changes.overlay.json"]
effective_spec = "effective.json"
bindings_metadata = "bindings.json"

[features]
enable_async_client = true
enable_sse_client = false

[http_client.tracing]
enabled = false

[client]
operations = ["render"]
prune_models = true
"#,
    )
    .unwrap();

    let output = config_dir.join("generated");
    let preview = generate(&cwd, &config, &["--dry-run", "--json"]);
    success(&preview);
    assert!(!output.exists(), "dry-run must not write either sidecar");
    let summary: Value = serde_json::from_slice(&preview.stdout).unwrap();
    let files = summary["files"].as_array().unwrap();
    assert!(files.contains(&json!("effective.json")));
    assert!(files.contains(&json!("bindings.json")));

    success(&generate(&cwd, &config, &["--quiet"]));
    let effective: Value =
        serde_json::from_slice(&std::fs::read(output.join("effective.json")).unwrap()).unwrap();
    assert!(effective["paths"].get("/legacy").is_none());
    assert_eq!(
        effective["servers"][0]["url"],
        "https://effective.example.test"
    );
    assert!(
        effective["paths"]["/render"]["post"]["responses"]["200"]["content"]
            .get("application/vnd.report+json")
            .is_some()
    );

    let client = std::fs::read_to_string(output.join("client.rs")).unwrap();
    let types = std::fs::read_to_string(output.join("types.rs")).unwrap();
    let bindings = std::fs::read(output.join("bindings.json")).unwrap();
    let metadata: Value = serde_json::from_slice(&bindings).unwrap();
    let metadata_text = metadata.to_string();
    assert!(client.contains("https://effective.example.test"));
    assert!(!client.contains("pub async fn legacy"));
    assert!(
        types.contains("pub alternate: i64"),
        "alternate inline schema was pruned: {types}"
    );
    assert!(!types.contains("pub struct Unused"));
    assert!(metadata_text.contains("application/vnd.report+json"));
    assert!(metadata_text.contains("/paths/~1render/post"));
    assert!(metadata_text.contains("alternate"));
    assert!(!metadata_text.contains("/paths/~1legacy/get"));
    assert!(!metadata_text.contains("Unused"));
    assert!(!metadata_text.contains("unrelated-working-directory"));

    success(&generate(&cwd, &config, &["--check", "--quiet"]));
    success(&generate(&cwd, &config, &["--quiet"]));
    assert_eq!(
        std::fs::read(output.join("bindings.json")).unwrap(),
        bindings
    );

    // Verification must detect stale sidecars without overwriting them.
    for artifact in ["bindings.json", "effective.json"] {
        let path = output.join(artifact);
        let expected = std::fs::read(&path).unwrap();
        std::fs::write(&path, "stale artifact\n").unwrap();
        let checked = generate(&cwd, &config, &["--check", "--quiet"]);
        assert!(!checked.status.success());
        assert!(String::from_utf8_lossy(&checked.stderr).contains(artifact));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "stale artifact\n");
        std::fs::write(&path, expected).unwrap();
    }
}
