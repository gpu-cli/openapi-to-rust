//! End-to-end tests: `[server]` TOML section → selector resolution
//! against a real spec.

use openapi_to_rust::ConfigFile;
use openapi_to_rust::SchemaAnalyzer;
use openapi_to_rust::server::{OperationIndex, Selector, resolve};
use std::io::Write;
use tempfile::NamedTempFile;

fn write_config(spec_path: &std::path::Path, server_block: &str) -> NamedTempFile {
    let spec_path = spec_path.canonicalize().expect("spec path canonicalizes");
    let content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]

{server_block}
"#,
        spec_path.display()
    );
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, "{content}").unwrap();
    f
}

fn load_index(spec_path: &std::path::Path) -> OperationIndex {
    let body = std::fs::read_to_string(spec_path).unwrap();
    let value: serde_json::Value = if spec_path.extension().and_then(|e| e.to_str()) == Some("yaml")
        || spec_path.extension().and_then(|e| e.to_str()) == Some("yml")
    {
        openapi_to_rust::cli::yaml_to_json_value(&body).unwrap()
    } else {
        openapi_to_rust::cli::json_from_str_lossy(&body).unwrap()
    };
    let mut analyzer = SchemaAnalyzer::new(value).unwrap();
    let analysis = analyzer.analyze().unwrap();
    OperationIndex::from_analysis(&analysis)
}

#[test]
fn server_section_parses_and_validates() {
    let spec_path = std::path::Path::new("specs/openai.yaml");
    let config_file = write_config(
        spec_path,
        r#"[server]
framework = "axum"
operations = ["createResponse", "tag:Embeddings", "POST /v1/messages"]
"#,
    );
    let cfg = ConfigFile::load(config_file.path()).expect("config loads");
    let server = cfg.server.expect("[server] present");
    assert_eq!(server.framework, "axum");
    let selectors = server.parsed_selectors().expect("selectors parse");
    assert_eq!(selectors.len(), 3);
    assert_eq!(selectors[0], Selector::OperationId("createResponse".into()));
    assert_eq!(selectors[1], Selector::Tag("Embeddings".into()));
    assert_eq!(
        selectors[2],
        Selector::MethodPath {
            method: "POST".into(),
            path: "/v1/messages".into()
        }
    );
}

#[test]
fn server_section_absent_is_valid() {
    let spec_path = std::path::Path::new("specs/openai.yaml");
    let config_file = write_config(spec_path, "");
    let cfg = ConfigFile::load(config_file.path()).expect("config loads");
    assert!(cfg.server.is_none());
}

#[test]
fn server_section_rejects_unknown_framework() {
    let spec_path = std::path::Path::new("specs/openai.yaml");
    let config_file = write_config(
        spec_path,
        r#"[server]
framework = "rocket"
operations = []
"#,
    );
    let err = ConfigFile::load(config_file.path()).expect_err("should reject");
    let msg = format!("{err}");
    assert!(msg.contains("axum"), "error should mention axum: {msg}");
}

#[test]
fn empty_operations_is_valid() {
    let spec_path = std::path::Path::new("specs/openai.yaml");
    let config_file = write_config(
        spec_path,
        r#"[server]
framework = "axum"
operations = []
"#,
    );
    let cfg = ConfigFile::load(config_file.path()).expect("config loads");
    let server = cfg.server.expect("[server] present");
    assert!(server.operations.is_empty());
    assert!(server.parsed_selectors().expect("parse").is_empty());
}

#[test]
fn resolves_create_response_against_openai_spec() {
    let index = load_index(std::path::Path::new("specs/openai.yaml"));
    let r = resolve(&[Selector::OperationId("createResponse".into())], &index).unwrap();
    assert_eq!(r.operations.len(), 1);
    assert_eq!(r.operations[0].operation_id, "createResponse");
    assert!(
        r.operations[0].supports_streaming,
        "createResponse declares SSE"
    );
}

#[test]
fn resolves_messages_post_against_anthropic_spec() {
    let index = load_index(std::path::Path::new("specs/anthropic.yaml"));
    let r = resolve(&[Selector::OperationId("messages_post".into())], &index).unwrap();
    assert_eq!(r.operations.len(), 1);
    assert_eq!(r.operations[0].operation_id, "messages_post");
}

#[test]
fn op_id_typo_suggests_correct_id() {
    let index = load_index(std::path::Path::new("specs/openai.yaml"));
    let err = resolve(&[Selector::OperationId("createRespons".into())], &index).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("createResponse"),
        "expected suggestion 'createResponse' in: {msg}"
    );
}

#[test]
fn tag_typo_suggests_correct_tag() {
    let index = load_index(std::path::Path::new("specs/openai.yaml"));
    let err = resolve(&[Selector::Tag("Embedding".into())], &index).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("Embeddings"),
        "expected suggestion 'Embeddings' in: {msg}"
    );
}
