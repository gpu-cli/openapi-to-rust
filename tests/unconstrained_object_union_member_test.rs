//! gh#70: the `object` member of a `type: [...]` union shares one
//! `SchemaDetails` with every other member, so it may carry no object shape at
//! all. Projecting that as a struct produced a closed, empty one: arbitrary
//! objects deserialized fine and then serialized back as `{}`, silently
//! dropping every key. The unconstrained member must use a map carrier, while
//! a member the spec *does* shape keeps its generated struct.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::process::Command;

/// The reproduction from gh#70 plus the two shapes that must not change:
/// an object member with declared properties, and one closed by
/// `additionalProperties: false`.
fn union_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "object union", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            "Unconstrained": {
                "type": "object",
                "required": ["payload"],
                "properties": { "payload": {
                    "type": ["string", "number", "boolean", "object", "array", "null"],
                    "items": {}
                } }
            },
            "Shaped": {
                "type": "object",
                "required": ["payload"],
                "properties": { "payload": {
                    "type": ["string", "object"],
                    "properties": { "id": { "type": "string" } }
                } }
            },
            "Closed": {
                "type": "object",
                "required": ["payload"],
                "properties": { "payload": {
                    "type": ["string", "object"],
                    "additionalProperties": false
                } }
            }
        } }
    })
}

fn generate(spec: Value, output_dir: std::path::PathBuf) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("parse object union spec");
    let mut analysis = analyzer.analyze().expect("analyze object union spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "object_union".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    generator
        .generate(&mut analysis)
        .expect("generate object union models")
}

#[test]
fn unconstrained_object_member_uses_a_map_carrier() {
    let temp = tempfile::TempDir::new().expect("temporary output directory");
    let generated = generate(union_spec(), temp.path().join("generated"));

    // rustfmt wraps the alias across lines, so compare without whitespace.
    let dense = generated
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    assert!(
        dense.contains(
            "pubtypeUnconstrainedPayloadObject=std::collections::BTreeMap<String,serde_json::Value"
        ),
        "an unconstrained object member needs a map carrier to keep its keys:\n{generated}"
    );
    assert!(
        !generated.contains("struct UnconstrainedPayloadObject"),
        "the unconstrained object member must not become an empty struct:\n{generated}"
    );
    // The map only matches objects, so the other members still get their own
    // variants — `serde_json::Value` would swallow the array and the scalars.
    for expected in [
        "String(String)",
        "Number(f64)",
        "Boolean(bool)",
        "UnconstrainedPayloadObject(UnconstrainedPayloadObject)",
        "UnconstrainedPayloadArray(UnconstrainedPayloadArray)",
    ] {
        assert!(
            generated.contains(expected),
            "the union must keep its `{expected}` member:\n{generated}"
        );
    }
}

#[test]
fn shaped_and_closed_object_members_keep_their_structs() {
    let temp = tempfile::TempDir::new().expect("temporary output directory");
    let generated = generate(union_spec(), temp.path().join("generated"));

    assert!(
        generated.contains("struct ShapedPayloadObject")
            && generated.contains("pub id: Option<String>"),
        "declared properties must still produce a struct:\n{generated}"
    );
    assert!(
        generated.contains("struct ClosedPayloadObject"),
        "`additionalProperties: false` is a closed empty object, not a map:\n{generated}"
    );
    assert!(
        !generated.contains("pub type ClosedPayloadObject"),
        "`additionalProperties: false` must not gain a map carrier:\n{generated}"
    );
}

#[test]
fn generated_object_union_round_trips_arbitrary_keys() {
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    let output_dir = temp.path().join("src/generated");
    let mut analyzer = SchemaAnalyzer::new(union_spec()).expect("parse object union spec");
    let mut analysis = analyzer.analyze().expect("analyze object union spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "object_union".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator
        .generate_all(&mut analysis)
        .expect("generate object union models");
    generator
        .write_files(&result)
        .expect("write object union models");

    let dependency_fragment =
        std::fs::read_to_string(temp.path().join("src/generated/REQUIRED_DEPS.toml"))
            .expect("generated dependency fragment");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "unconstrained-object-union-smoke"
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
    use serde_json::{Value, json};

    fn round_trip(input: Value) {
        let hydrated: generated::Unconstrained =
            serde_json::from_value(input.clone()).expect("hydrate valid JSON");
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
    }

    #[test]
    fn every_member_of_the_union_preserves_its_value() {
        // gh#70: the object payload used to come back out as `{}`.
        round_trip(json!({"payload": {"key": "value"}}));
        round_trip(json!({"payload": {"nested": {"a": [1, 2]}, "b": null}}));
        round_trip(json!({"payload": {}}));
        round_trip(json!({"payload": "text"}));
        round_trip(json!({"payload": 1.5}));
        round_trip(json!({"payload": true}));
        round_trip(json!({"payload": ["a", 1]}));
        round_trip(json!({"payload": []}));
    }

    #[test]
    fn the_object_member_is_the_one_that_matches_an_object() {
        let hydrated: generated::Unconstrained =
            serde_json::from_value(json!({"payload": {"key": "value"}})).expect("hydrate object");
        assert!(matches!(
            hydrated.payload,
            Some(generated::UnconstrainedPayload::UnconstrainedPayloadObject(_))
        ));
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
                .join("target/unconstrained-object-union-smoke"),
        )
        .output()
        .expect("run generated object union round-trip tests");
    assert!(
        output.status.success(),
        "generated object union round-trip tests failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
