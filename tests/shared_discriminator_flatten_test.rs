use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn shared_discriminator_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "shared flattened discriminator", "version": "1" },
        "paths": {},
        "components": { "schemas": {
            "NonEmptyString": { "type": "string", "minLength": 1 },
            "TextPart": {
                "type": "object",
                "additionalProperties": false,
                "required": ["type", "text"],
                "properties": {
                    "type": { "const": "text" },
                    "text": { "type": "string" }
                }
            },
            "FilePart": {
                "type": "object",
                "additionalProperties": false,
                "required": ["type", "url"],
                "properties": {
                    "type": { "const": "file" },
                    "url": { "type": "string" }
                }
            },
            "MessagePart": {
                "type": "object",
                "required": ["type"],
                "properties": {
                    "type": { "$ref": "#/components/schemas/NonEmptyString" }
                },
                "oneOf": [
                    { "$ref": "#/components/schemas/TextPart" },
                    { "$ref": "#/components/schemas/FilePart" }
                ],
                "discriminator": {
                    "propertyName": "type",
                    "mapping": {
                        "text": "#/components/schemas/TextPart",
                        "file": "#/components/schemas/FilePart"
                    }
                }
            }
        } }
    })
}

#[test]
fn sibling_and_variant_deserialize_from_one_complete_object() {
    let mut analysis = SchemaAnalyzer::new(shared_discriminator_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("spec should generate");
    let compact = generated.split_whitespace().collect::<String>();
    assert!(compact.contains("struct__MessagePartBase"));
    assert!(compact.contains("impl<'de>serde::Deserialize<'de>forMessagePart"));
    assert!(!compact.contains("#[serde(flatten)]pubvariant:MessagePartVariant"));

    generated.push_str(
        r#"
#[cfg(test)]
mod shared_discriminator_runtime {
    use super::MessagePart;

    #[test]
    fn shared_tag_is_visible_to_both_halves_and_serializes_once() {
        for input in [
            serde_json::json!({"type": "text", "text": "hello"}),
            serde_json::json!({"type": "file", "url": "https://example.test/file"}),
        ] {
            let hydrated: MessagePart = serde_json::from_value(input.clone()).unwrap();
            let output = serde_json::to_value(hydrated).unwrap();
            assert_eq!(output, input);
            let stable: MessagePart = serde_json::from_value(output.clone()).unwrap();
            assert_eq!(serde_json::to_value(stable).unwrap(), output);
        }
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "shared-discriminator-flatten-smoke"
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
                .join("target/shared-discriminator-flatten-smoke"),
        )
        .output()
        .expect("run generated shared-discriminator test");
    assert!(
        output.status.success(),
        "generated shared-discriminator round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
