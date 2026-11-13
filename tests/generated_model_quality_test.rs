use openapi_to_rust::{CodeGenerator, DerivesConfig, GeneratorConfig, SchemaAnalyzer};
use quote::ToTokens;
use serde_json::json;
use std::{fs, process::Command};

#[test]
fn model_derives_and_builders_compile_and_pass_default_clippy() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "model quality", "version": "1"},
        "paths": {"/orders": {"post": {
            "operationId": "createOrder",
            "requestBody": {"required": true, "content": {"application/json": {
                "schema": {"$ref": "#/components/schemas/Order"}
            }}},
            "responses": {"204": {"description": "Created"}}
        }}},
        "components": {"schemas": {
            "Cat": {"type": "object", "required": ["meows"],
                "properties": {"meows": {"type": "boolean"}}},
            "Dog": {"type": "object", "required": ["barks"],
                "properties": {"barks": {"type": "boolean"}}},
            "Pet": {"oneOf": [
                {"$ref": "#/components/schemas/Cat"}, {"$ref": "#/components/schemas/Dog"}
            ]},
            "Order": {"type": "object", "required": ["id", "quantity", "pet"],
                "additionalProperties": true,
                "properties": {
                    "id": {"type": "string"}, "quantity": {"type": "integer"},
                    "pet": {"$ref": "#/components/schemas/Pet"},
                    "note": {"type": "string"},
                    "nullable_note": {"type": ["string", "null"]},
                    "status": {"type": ["string", "null"], "enum": ["active", null, "archived"]}
                }}
        }}
    });
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let source = CodeGenerator::new(GeneratorConfig {
        derives: DerivesConfig {
            models: vec!["PartialEq".into()],
            ..Default::default()
        },
        ..Default::default()
    })
    .generate(&mut analysis)
    .unwrap();
    assert!(!source.contains("#struct_name"));
    assert!(source.contains("Dependency-free builder for [`Order`]."));
    let ast = syn::parse_file(&source).unwrap();
    let builder = ast
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Struct(item) if item.ident == "OrderBuilder" => Some(item),
            _ => None,
        })
        .unwrap();
    let attrs = builder
        .attrs
        .iter()
        .map(|a| a.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(attrs.contains("must_use"));
    assert!(!attrs.contains("PartialEq"));

    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    fs::write(temp.path().join("src/types.rs"), source).unwrap();
    fs::write(
        temp.path().join("src/lib.rs"),
        r#"
pub mod types;
#[cfg(test)]
mod tests {
    use super::types::*;
    #[test]
    fn compare_models_and_exercise_builders() {
        let wire = serde_json::json!({"id":"1", "quantity":2, "pet":{"meows":true}});
        let first: Order = serde_json::from_value(wire.clone()).unwrap();
        let second: Order = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(first, second);
        assert_eq!(serde_json::to_value(first).unwrap(), wire);
        let built = Order::builder("1".into(), Pet::Cat(Cat { meows: true }), 2)
            .note("note".into()).nullable_note("value".into())
            .nullable_note_null().nullable_note_absent()
            .additional_properties(Default::default()).build();
        assert_eq!(built.note.as_deref(), Some("note"));
    }
}
"#,
    )
    .unwrap();
    for edition in ["2021", "2024"] {
        fs::write(
            temp.path().join("Cargo.toml"),
            format!(
                r#"
[package]
name = "generated-model-quality-smoke"
version = "0.0.0"
edition = "{edition}"
[workspace]
[dependencies]
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
            ),
        )
        .unwrap();
        for args in [
            vec!["test", "--quiet", "--offline"],
            vec![
                "clippy",
                "--quiet",
                "--offline",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ] {
            let output = Command::new("cargo")
                .args(&args)
                .current_dir(temp.path())
                .env(
                    "CARGO_TARGET_DIR",
                    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("target/generated-model-quality-smoke"),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "edition {edition}, {args:?}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
