//! A union branch typed `serde_json::Value` matches every JSON shape, so it
//! claims values that belong to a later branch: with
//! `anyOf: [{type: object}, {type: string}]` a JSON string deserializes into
//! the object branch and never reaches `String(String)`. A branch that
//! declares `type: object` gets a map carrier instead — equally lossless, and
//! it matches only objects.
//!
//! The negative case is the point of the split: a branch that declares no type
//! at all really does admit any JSON and must keep `serde_json::Value`.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::process::Command;

fn spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "opaque object branch", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            // The object branch is declared first, so it is the one serde tries
            // first and the one that used to swallow the string.
            "ObjectFirst": {
                "type": "object",
                "properties": { "p": { "anyOf": [
                    { "type": "object" },
                    { "type": "string" }
                ] } }
            },
            // No `type` at all: this branch is genuinely "any JSON".
            "AnyBranch": {
                "type": "object",
                "properties": { "p": { "anyOf": [
                    { "type": "string" },
                    { }
                ] } }
            }
        } }
    })
}

fn generate(spec: Value, output_dir: std::path::PathBuf) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("parse branch spec");
    let mut analysis = analyzer.analyze().expect("analyze branch spec");
    CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "opaque_object_branch".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("generate branch models")
}

#[test]
fn an_object_branch_uses_a_map_and_an_untyped_branch_stays_a_value() {
    let temp = tempfile::TempDir::new().expect("temporary output directory");
    let generated = generate(spec(), temp.path().join("generated"));
    let dense = generated
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();

    assert!(
        dense.contains("=std::collections::BTreeMap<String,serde_json::Value"),
        "a branch declaring `type: object` needs a map carrier:\n{generated}"
    );
    assert!(
        dense.contains("=serde_json::Value;"),
        "a branch declaring no type at all must stay `serde_json::Value`:\n{generated}"
    );
}

#[test]
fn generated_branches_select_the_variant_the_wire_shape_belongs_to() {
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    let output_dir = temp.path().join("src/generated");
    let mut analyzer = SchemaAnalyzer::new(spec()).expect("parse branch spec");
    let mut analysis = analyzer.analyze().expect("analyze branch spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "opaque_object_branch".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator
        .generate_all(&mut analysis)
        .expect("generate branch models");
    generator.write_files(&result).expect("write branch models");

    let dependency_fragment =
        std::fs::read_to_string(temp.path().join("src/generated/REQUIRED_DEPS.toml"))
            .expect("generated dependency fragment");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "opaque-object-branch-smoke"
version = "0.0.0"
edition = "2024"
publish = false

{dependency_fragment}
"#
        ),
    )
    .expect("write scratch manifest");
    std::fs::write(
        temp.path().join("src/lib.rs"),
        r#"pub mod generated;

#[cfg(test)]
mod tests {
    use super::generated;
    use serde_json::json;

    #[test]
    fn a_string_reaches_the_string_branch_past_the_object_branch() {
        let hydrated: generated::ObjectFirst =
            serde_json::from_value(json!({"p": "hello"})).expect("hydrate a string");
        assert!(
            matches!(hydrated.p, Some(generated::ObjectFirstP::String(_))),
            "a string must not be claimed by the object branch: {:?}",
            hydrated.p
        );
    }

    #[test]
    fn an_object_still_reaches_the_object_branch_with_its_keys() {
        let input = json!({"p": {"a": 1, "b": [2, 3]}});
        let hydrated: generated::ObjectFirst =
            serde_json::from_value(input.clone()).expect("hydrate an object");
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
    }
}
"#,
    )
    .expect("write scratch tests");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/opaque-object-branch-smoke"),
        )
        .output()
        .expect("run generated branch tests");
    assert!(
        output.status.success(),
        "generated branch selection tests failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
