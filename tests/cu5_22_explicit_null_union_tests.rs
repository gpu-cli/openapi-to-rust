use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::process::Command;

fn union_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "explicit null union", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            "Known": {
                "type": "object",
                "additionalProperties": false,
                "required": ["id"],
                "properties": { "id": { "type": "string" } }
            },
            "AnyRequired": {
                "type": "object",
                "required": ["value"],
                "properties": { "value": { "anyOf": [
                    { "$ref": "#/components/schemas/Known" },
                    { "nullable": true }
                ] } }
            },
            "AnyOptionalObject": {
                "type": "object",
                "properties": { "value": { "anyOf": [
                    { "type": "object", "nullable": true },
                    { "$ref": "#/components/schemas/Known" }
                ] } }
            },
            "OneRequired": {
                "type": "object",
                "required": ["value"],
                "properties": { "value": { "oneOf": [
                    { "$ref": "#/components/schemas/Known" },
                    { "nullable": true }
                ] } }
            },
            "OneOptionalObject": {
                "type": "object",
                "properties": { "value": { "oneOf": [
                    { "type": "object", "nullable": true },
                    { "$ref": "#/components/schemas/Known" }
                ] } }
            },
            "ExplicitRequired": {
                "type": "object",
                "required": ["value"],
                "properties": { "value": { "anyOf": [
                    { "$ref": "#/components/schemas/Known" },
                    { "type": "null" }
                ] } }
            },
            "ExplicitOptional": {
                "type": "object",
                "properties": { "value": { "oneOf": [
                    { "const": null },
                    { "$ref": "#/components/schemas/Known" }
                ] } }
            },
            "ThreeBranches": {
                "anyOf": [
                    { "$ref": "#/components/schemas/Known" },
                    { "type": "null" },
                    { "type": "array", "items": { "type": "integer" } }
                ]
            }
        } }
    })
}

fn generate(spec: Value, output_dir: std::path::PathBuf) -> (CodeGenerator, String) {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("parse synthetic union spec");
    let mut analysis = analyzer.analyze().expect("analyze synthetic union spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "explicit_null_union".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let generated = generator
        .generate(&mut analysis)
        .expect("generate synthetic union models");
    (generator, generated)
}

#[test]
fn nullable_true_branches_remain_real_union_alternatives() {
    let temp = tempfile::TempDir::new().expect("temporary output directory");
    let (_, generated) = generate(union_spec(), temp.path().join("generated"));

    for expected in [
        "pub struct AnyRequired {\n    pub value: AnyRequiredValue,",
        "pub value: Option<AnyOptionalObjectValue>",
        "pub struct OneRequired {\n    pub value: OneRequiredValue,",
        "pub value: Option<OneOptionalObjectValue>",
    ] {
        assert!(
            generated.contains(expected),
            "nullable:true must keep both branches in `{expected}`:\n{generated}"
        );
    }
    assert!(
        generated.contains("serde_json::Value"),
        "an unconstrained branch needs a dynamic JSON carrier:\n{generated}"
    );
    assert!(
        generated.contains("pub enum ThreeBranches")
            && generated.contains("Known(Known)")
            && generated.contains("Vec<i64>"),
        "three branches must not collapse to an Option reference:\n{generated}"
    );
}

#[test]
fn generated_required_and_optional_unions_round_trip_every_valid_shape() {
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    let output_dir = temp.path().join("src/generated");
    let mut analyzer = SchemaAnalyzer::new(union_spec()).expect("parse synthetic union spec");
    let mut analysis = analyzer.analyze().expect("analyze synthetic union spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "explicit_null_union".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator
        .generate_all(&mut analysis)
        .expect("generate synthetic union models");
    generator
        .write_files(&result)
        .expect("write synthetic union models");

    let dependency_fragment =
        std::fs::read_to_string(temp.path().join("src/generated/REQUIRED_DEPS.toml"))
            .expect("generated dependency fragment");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "cu5-22-explicit-null-union-smoke"
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
    use serde::{Serialize, de::DeserializeOwned};
    use serde_json::{Value, json};

    fn round_trip<T>(input: Value)
    where
        T: DeserializeOwned + Serialize,
    {
        let hydrated: T = serde_json::from_value(input.clone()).expect("hydrate valid JSON");
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
    }

    fn hydrate_optional_null<T>()
    where
        T: DeserializeOwned + Serialize,
    {
        let input = json!({"value": null});
        let hydrated: T = serde_json::from_value(input).expect("hydrate optional explicit null");
        let output = serde_json::to_value(hydrated).unwrap();
        assert!(
            output == json!({}) || output == json!({"value": null}),
            "an optional null may retain its presence bit or serialize as schema-valid absence"
        );
    }

    #[test]
    fn required_and_optional_union_fields_preserve_valid_json() {
        for value in [
            json!(7),
            json!([1, "two"]),
            Value::Null,
            json!({"free": "shape"}),
            json!({"id": "known"}),
        ] {
            round_trip::<generated::AnyRequired>(json!({"value": value}));
        }

        round_trip::<generated::AnyOptionalObject>(json!({}));
        hydrate_optional_null::<generated::AnyOptionalObject>();
        for value in [json!({"free": "shape"}), json!({"id": "known"})] {
            round_trip::<generated::AnyOptionalObject>(json!({"value": value}));
        }

        // The referenced object also matches the unconstrained branch, so it
        // is not valid under oneOf's exactly-one rule. The remaining shapes
        // each match exactly the unconstrained branch.
        for value in [json!(7), json!([1, "two"]), Value::Null, json!({"free": "shape"})] {
            round_trip::<generated::OneRequired>(json!({"value": value}));
        }
        round_trip::<generated::OneOptionalObject>(json!({}));
        hydrate_optional_null::<generated::OneOptionalObject>();
        for value in [json!({"free": "shape"})] {
            round_trip::<generated::OneOptionalObject>(json!({"value": value}));
        }

        round_trip::<generated::ExplicitRequired>(json!({"value": null}));
        round_trip::<generated::ExplicitRequired>(json!({"value": {"id": "known"}}));
        round_trip::<generated::ExplicitOptional>(json!({}));
        hydrate_optional_null::<generated::ExplicitOptional>();
        round_trip::<generated::ExplicitOptional>(json!({"value": {"id": "known"}}));
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
                .join("target/cu5-22-explicit-null-union-smoke"),
        )
        .output()
        .expect("run generated union round-trip tests");
    assert!(
        output.status.success(),
        "generated union round-trip tests failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn microsoft_graph_nullable_object_spelling_still_parses_and_generates() {
    let spec = json!({
        "openapi": "3.0.1",
        "info": { "title": "OData compatibility", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            "User": {
                "type": "object",
                "properties": { "id": { "type": "string" } }
            },
            "DirectoryObject": {
                "type": "object",
                "properties": { "user": { "anyOf": [
                    { "$ref": "#/components/schemas/User" },
                    { "type": "object", "nullable": true }
                ] } }
            }
        } }
    });
    let temp = tempfile::TempDir::new().expect("temporary output directory");
    let (_, generated) = generate(spec, temp.path().join("generated"));
    assert!(
        !generated.contains("pub user: Option<User>"),
        "the legacy spelling must parse without discarding its object branch:\n{generated}"
    );
}
