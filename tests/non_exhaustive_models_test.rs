use openapi_to_rust::{CodeGenerator, ConfigFile, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::process::Command;

fn spec(add_optional: bool) -> Value {
    let mut spec = json!({
        "openapi":"3.1.0", "info":{"title":"model evolution","version":"1"},
        "paths": {"/items": {"post": {
            "operationId":"createItem",
            "requestBody":{"required":true,"content":{"application/json":{"schema":{"$ref":"#/components/schemas/CreateItem"}}}},
            "responses":{"204":{"description":"created"}}
        }}},
        "components":{"schemas":{
            "CreateItem":{"type":"object","additionalProperties":false,"required":["nested"],"properties":{"nested":{"$ref":"#/components/schemas/Nested"}}},
            "Nested":{"type":"object","additionalProperties":false,"required":["name"],"properties":{"name":{"type":"string"}}},
            "Options":{"type":"object","additionalProperties":false,"properties":{"label":{"type":"string"}}},
            "SourceField": {
                "type":"object", "properties":{"name":{"type":"string"}}, "required":["name"],
                "discriminator":{"propertyName":"type"},
                "anyOf":[
                    {"title":"Small", "type":"object", "properties":{"type":{"type":"string","enum":["small"]}}, "required":["type"]},
                    {"title":"Large", "type":"object", "properties":{"type":{"type":"string","enum":["large"]}}, "required":["type"]}
                ]
            }
        }}
    });
    if add_optional {
        for name in ["CreateItem", "Nested", "Options"] {
            spec["components"]["schemas"][name]["properties"]["added"] = json!({"type":"string"});
        }
    }
    spec
}

fn generate(spec: Value, enabled: bool, output: &std::path::Path) -> String {
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: output.into(),
        enable_async_client: false,
        enable_sse_client: false,
        non_exhaustive_models: enabled,
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    let types = result
        .files
        .iter()
        .find(|file| file.path == std::path::Path::new("types.rs"))
        .unwrap()
        .content
        .clone();
    generator.write_files(&result).unwrap();
    types
}

#[test]
fn option_round_trips_and_defaults_to_existing_output() {
    let base = r#"[generator]
spec_path = "openapi.json"
output_dir = "generated"
module_name = "api"
[features]
enable_async_client = false
"#;
    let disabled: ConfigFile = toml::from_str(base).unwrap();
    assert!(!disabled.generator.non_exhaustive_models);
    assert!(!disabled.into_generator_config().non_exhaustive_models);
    let enabled: ConfigFile = toml::from_str(&base.replace(
        "module_name = \"api\"",
        "module_name = \"api\"\nnon_exhaustive_models = true",
    ))
    .unwrap();
    let roundtrip: ConfigFile = toml::from_str(&toml::to_string(&enabled).unwrap()).unwrap();
    assert!(roundtrip.generator.non_exhaustive_models);
    assert!(roundtrip.into_generator_config().non_exhaustive_models);
    let dir = tempfile::tempdir().unwrap();
    assert!(!generate(spec(false), false, dir.path()).contains("#[non_exhaustive]"));
}

#[test]
#[cfg(feature = "cli")]
fn cli_flag_enables_extensible_models() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("spec.json");
    let output_dir = dir.path().join("generated");
    std::fs::write(&input, serde_json::to_vec(&spec(false)).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_openapi-to-rust"))
        .args([
            "generate",
            input.to_str().unwrap(),
            "--types-only",
            "--non-exhaustive-models",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let types = std::fs::read_to_string(output_dir.join("types.rs")).unwrap();
    assert!(types.contains("#[non_exhaustive]"));
    assert!(types.contains("pub fn new(name: String) -> Self"));
}

#[test]
fn downstream_code_survives_optional_fields_on_root_nested_and_default_models() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "model_evolution"
version = "0.1.0"
edition = "2024"
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "pub mod generated;\n").unwrap();
    let main = r#"use model_evolution::generated::{CreateItem, Nested, Options, SourceField};
fn main() {
    let nested = Nested::new("example".to_owned());
    let Nested { name, .. } = nested.clone();
    assert_eq!(name, "example");
    let item = CreateItem::new(nested);
    assert_eq!(serde_json::to_value(item).unwrap(), serde_json::json!({"nested":{"name":"example"}}));
    let mut options = Options::default();
    options.label = Some("label".to_owned());
    assert_eq!(serde_json::to_value(options).unwrap(), serde_json::json!({"label":"label"}));
    let variant = serde_json::from_value(serde_json::json!({"type":"small"})).unwrap();
    let _ = SourceField::new("field".to_owned(), variant);
}
"#;
    std::fs::write(dir.path().join("src/main.rs"), main).unwrap();
    for added in [false, true] {
        let types = generate(spec(added), true, &dir.path().join("src/generated"));
        assert!(types.contains("#[non_exhaustive]"));
        let output = Command::new("cargo")
            .args(["run", "--quiet", "--offline"])
            .current_dir(dir.path())
            .env(
                "CARGO_TARGET_DIR",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("target/model-evolution-smoke"),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "added={added}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    // These patterns must be rejected for downstream crates, so additions stay compatible.
    std::fs::write(
        dir.path().join("src/main.rs"),
        r#"use model_evolution::generated::Nested;
fn main() { let _ = Nested { name: "example".to_owned(), added: None }; }
"#,
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["check", "--quiet", "--offline"])
        .current_dir(dir.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/model-evolution-smoke"),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-exhaustive"));

    std::fs::write(
        dir.path().join("src/main.rs"),
        r#"use model_evolution::generated::Nested;
fn main() { let Nested { name, added } = Nested::new("example".to_owned()); }
"#,
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["check", "--quiet", "--offline"])
        .current_dir(dir.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/model-evolution-smoke"),
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-exhaustive"));
}
