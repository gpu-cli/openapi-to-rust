//! `allowReserved: true` on a path parameter selects RFC 6570 reserved
//! expansion, so a value such as `XLON:LLOY` keeps its colon. Without it the
//! default segment encoder still percent-encodes every reserved character.
//! This is standard behavior in 3.2, and a compatibility extension in 3.0/3.1.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, analysis::SchemaAnalyzer};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::process::Command;

fn config() -> GeneratorConfig {
    GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    }
}

fn spec(allow_reserved: Option<bool>) -> Value {
    let mut param = json!({
        "name": "identifier",
        "in": "path",
        "required": true,
        "schema": {"type": "string"}
    });
    if let Some(flag) = allow_reserved {
        param["allowReserved"] = json!(flag);
    }
    json!({
        "openapi": "3.1.0",
        "info": {"title": "T", "version": "1.0.0"},
        "paths": {
            "/prices/{identifier}": {
                "get": {
                    "operationId": "getPrice",
                    "parameters": [param],
                    "responses": {"200": {"description": "ok"}}
                }
            }
        }
    })
}

fn generate_methods(spec: Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let analysis = analyzer.analyze().expect("analysis");
    CodeGenerator::new(config())
        .generate_operation_methods(&analysis)
        .to_string()
}

fn generate_client(spec: Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let analysis = analyzer.analyze().expect("analysis");
    CodeGenerator::new(config())
        .generate_http_client(&analysis)
        .expect("client generation")
}

fn encoder_source(client: &str, name: &str) -> String {
    let start = client.find(name).expect("encoder is emitted");
    let body = &client[start..];
    body[..body.find("\n}\n").expect("encoder body ends")].to_string()
}

#[test]
fn allow_reserved_path_parameter_uses_reserved_expansion() {
    for version in ["3.0.3", "3.1.0", "3.2.0"] {
        let mut input = spec(Some(true));
        input["openapi"] = json!(version);
        let code = generate_methods(input);
        assert!(
            code.contains("__pct_encode_path_reserved (identifier . as_ref ())"),
            "{version}: {code}"
        );
        assert!(!code.contains("__pct_encode_path_segment ("), "{code}");
    }
}

#[test]
fn default_path_parameter_keeps_segment_encoding() {
    for flag in [None, Some(false)] {
        let code = generate_methods(spec(flag));
        assert!(
            code.contains("__pct_encode_path_segment (identifier . as_ref ())"),
            "{code}"
        );
        assert!(!code.contains("__pct_encode_path_reserved ("), "{code}");
    }
}

#[test]
fn reserved_encoder_keeps_segment_safe_reserved_bytes_only() {
    let client = generate_client(spec(Some(true)));
    let reserved = encoder_source(&client, "fn __pct_encode_path_reserved");
    for kept in ["b':'", "b'@'", "b'['", "b';'", "b'='", "b'%'"] {
        assert!(
            reserved.contains(kept),
            "{kept} must pass through: {reserved}"
        );
    }
    for encoded in ["b'/'", "b'?'", "b'#'"] {
        assert!(
            !reserved.contains(encoded),
            "{encoded} must stay percent-encoded: {reserved}"
        );
    }
    let default = encoder_source(&client, "fn __pct_encode_path_segment");
    assert!(!default.contains("b':'"), "{default}");
}

#[test]
fn reserved_encoder_is_only_emitted_when_used() {
    for flag in [None, Some(false)] {
        let client = generate_client(spec(flag));
        assert!(!client.contains("__pct_encode_path_reserved"), "{client}");
        let methods = generate_methods(spec(flag));
        assert!(!methods.contains("__pct_encode_path_reserved"), "{methods}");
    }
}

#[test]
fn reserved_encoder_follows_the_selected_operation_scope() {
    let mut input = spec(Some(true));
    input["paths"]["/health"] = json!({
        "get": {
            "operationId": "health",
            "responses": {"204": {"description": "ok"}}
        }
    });
    let mut analysis = SchemaAnalyzer::new(input).unwrap().analyze().unwrap();
    for (operation, expected_count) in [("health", 0), ("getPrice", 1)] {
        let generator = CodeGenerator::new(GeneratorConfig {
            client: Some(openapi_to_rust::config::ClientSection {
                operations: vec![operation.to_string()],
                prune_models: false,
            }),
            ..config()
        });
        let result = generator.generate_all(&mut analysis).unwrap();
        let client = &result
            .files
            .iter()
            .find(|file| file.path.to_str() == Some("client.rs"))
            .unwrap()
            .content;
        assert_eq!(
            client.matches("fn __pct_encode_path_reserved(").count(),
            expected_count,
            "{operation}: {client}"
        );
    }
}

#[test]
fn low_level_and_full_clients_compile_and_execute_reserved_encoding() {
    let mut analysis = SchemaAnalyzer::new(spec(Some(true)))
        .unwrap()
        .analyze()
        .unwrap();
    let generator = CodeGenerator::new(config());
    let full_client = generator.generate_http_client(&analysis).unwrap();
    // Error types are emitted only by the full-client API. Reuse that preamble,
    // then compose the client and operation artifacts through the public APIs.
    let error_preamble = full_client.split_once("use reqwest_middleware").unwrap().0;
    let client_struct = generator.generate_http_client_struct();
    let methods = generator.generate_operation_methods(&analysis);
    let low_level = format!("{error_preamble}\n{client_struct}\n{methods}");
    let low_level = prettyplease::unparse(&syn::parse_file(&low_level).unwrap());
    for client in [&low_level, &full_client] {
        assert_eq!(client.matches("fn __pct_encode_path_reserved(").count(), 1);
    }

    let result = generator.generate_all(&mut analysis).unwrap();
    let artifacts = generator.output_artifacts(&result);
    let dependencies = &artifacts[&PathBuf::from("REQUIRED_DEPS.toml")];
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join("src")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            "[package]\nname = \"path-reserved-smoke\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n{dependencies}"
        ),
    )
    .unwrap();
    let checks = r#"
#[cfg(test)]
mod tests {
    use super::__pct_encode_path_reserved as encode;

    #[test]
    fn reserved_path_values() {
        assert_eq!(encode("XLON:LLOY"), "XLON:LLOY");
        assert_eq!(encode(":@[]!$&'()*+,;="), ":@[]!$&'()*+,;=");
        assert_eq!(encode("a/b?c#d"), "a%2Fb%3Fc%23d");
        assert_eq!(encode("%3a%2F%zz%"), "%3a%2F%25zz%25");
        assert_eq!(encode("é 😀"), "%C3%A9%20%F0%9F%98%80");
    }
}
"#;
    std::fs::write(
        temp.path().join("src/low_level.rs"),
        format!("{low_level}\n{checks}"),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/full_client.rs"),
        format!("{full_client}\n{checks}"),
    )
    .unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "#![allow(dead_code, unused_imports)]\nmod types {}\nmod low_level;\nmod full_client;\n",
    )
    .unwrap();

    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/path-reserved-smoke"),
        )
        .env(
            "CARGO_BUILD_BUILD_DIR",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/path-reserved-smoke/build"),
        )
        .env("CC_aarch64_apple_darwin", "cc")
        .env("CXX_aarch64_apple_darwin", "c++")
        .env("CC", "cc")
        .env("CXX", "c++")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated clients failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
