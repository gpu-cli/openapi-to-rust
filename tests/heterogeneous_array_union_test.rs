use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{collections::HashSet, fs, process::Command};

fn heterogeneous_array_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "heterogeneous array union", "version": "1" },
        "paths": {},
        "components": { "schemas": {
            "CompletionPrompt": {
                "anyOf": [
                    { "type": "string" },
                    { "type": "array", "items": { "type": "string" } },
                    { "type": "array", "items": { "type": "integer" } },
                    {
                        "type": "array",
                        "items": {
                            "type": "array",
                            "items": { "type": "integer" }
                        }
                    }
                ]
            }
        } }
    })
}

#[test]
fn heterogeneous_array_branches_have_distinct_deterministic_targets() {
    let analyze = || {
        SchemaAnalyzer::new(heterogeneous_array_spec())
            .expect("spec should parse")
            .analyze()
            .expect("spec should analyze")
    };
    let first = analyze();
    let second = analyze();
    let targets = |analysis: &openapi_to_rust::SchemaAnalysis| {
        let SchemaType::Union { variants, .. } = &analysis.schemas["CompletionPrompt"].schema_type
        else {
            panic!("CompletionPrompt should be a union");
        };
        variants
            .iter()
            .map(|variant| variant.target.clone())
            .collect::<Vec<_>>()
    };
    let first_targets = targets(&first);
    assert_eq!(first_targets, targets(&second));
    assert_eq!(first_targets.len(), 4);
    assert_eq!(
        first_targets.iter().collect::<HashSet<_>>().len(),
        first_targets.len(),
        "no array branch may overwrite another alias: {first_targets:?}"
    );
}

#[test]
fn generated_flat_and_nested_array_branches_round_trip() {
    let mut analysis = SchemaAnalyzer::new(heterogeneous_array_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("spec should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod heterogeneous_array_runtime {
    use super::CompletionPrompt;

    #[test]
    fn every_wire_shape_survives() {
        for input in [
            serde_json::json!("prompt"),
            serde_json::json!(["one", "two"]),
            serde_json::json!([1, 2, 3]),
            serde_json::json!([[1, 2], [3, 4]]),
        ] {
            let hydrated: CompletionPrompt = serde_json::from_value(input.clone()).unwrap();
            assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
        }
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "heterogeneous-array-union-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    fs::create_dir(temp.path().join("src")).expect("create scratch source");
    fs::write(temp.path().join("src/lib.rs"), generated).expect("write generated source");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/heterogeneous-array-union-smoke"),
        )
        .output()
        .expect("run generated heterogeneous-array test");
    assert!(
        output.status.success(),
        "generated heterogeneous array union failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
