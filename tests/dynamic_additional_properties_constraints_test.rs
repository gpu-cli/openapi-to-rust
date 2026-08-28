use openapi_to_rust::analysis::{ObjectAdditionalProperties, SchemaType};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn dynamic_object_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "dynamic object constraints", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "ConstrainedMetric": {
                    "type": "object",
                    "minProperties": 2,
                    "maxProperties": 3,
                    "properties": {
                        "timestamp": { "type": "integer" },
                        "metric": { "type": "number" }
                    }
                },
                "ExampleDriven": {
                    "type": "object",
                    "properties": {
                        "placeholder": { "type": "string" }
                    },
                    "example": {
                        "actual_metric_name": 42
                    }
                },
                "ExamplesDriven": {
                    "type": "object",
                    "properties": {
                        "placeholder": { "type": "boolean" }
                    },
                    "examples": [
                        { "first_dynamic_name": true },
                        { "second_dynamic_name": "value" }
                    ]
                },
                "Closed": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["known"],
                    "properties": {
                        "known": { "type": "string" }
                    }
                },
                "Typed": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "properties": {
                        "known": { "type": "string" }
                    }
                }
            }
        }
    })
}

fn analyze() -> openapi_to_rust::SchemaAnalysis {
    SchemaAnalyzer::new(dynamic_object_spec())
        .expect("dynamic object spec should parse")
        .analyze()
        .expect("dynamic object spec should analyze")
}

fn additional_properties<'a>(
    analysis: &'a openapi_to_rust::SchemaAnalysis,
    name: &str,
) -> &'a ObjectAdditionalProperties {
    let SchemaType::Object {
        additional_properties,
        ..
    } = &analysis.schemas[name].schema_type
    else {
        panic!("{name} should analyze as an object");
    };
    additional_properties
}

#[test]
fn constraints_and_examples_promote_omitted_additional_properties_to_a_carrier() {
    let analysis = analyze();
    for name in ["ConstrainedMetric", "ExampleDriven", "ExamplesDriven"] {
        assert!(
            matches!(
                additional_properties(&analysis, name),
                ObjectAdditionalProperties::Untyped
            ),
            "{name} must retain undeclared keys"
        );
    }
    assert!(matches!(
        additional_properties(&analysis, "Closed"),
        ObjectAdditionalProperties::Forbidden
    ));
    assert!(matches!(
        additional_properties(&analysis, "Typed"),
        ObjectAdditionalProperties::Typed { .. }
    ));
}

#[test]
fn generated_dynamic_object_members_round_trip_without_breaking_typed_or_closed_controls() {
    let mut analysis = analyze();
    let code = CodeGenerator::new(GeneratorConfig {
        module_name: "dynamic_object_constraints".into(),
        enable_async_client: false,
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("dynamic object types should generate");
    let temp = tempfile::TempDir::new().expect("scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(temp.path().join("src/generated.rs"), code).expect("generated module");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "dynamic-additional-properties-constraints-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("scratch manifest");
    std::fs::write(
        temp.path().join("src/main.rs"),
        r##"#![allow(dead_code)]
mod generated;

fn round_trip<T>(input: serde_json::Value)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let hydrated: T = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
}

fn main() {
    round_trip::<generated::ConstrainedMetric>(serde_json::json!({
        "timestamp": 1623159320,
        "edge_status_2xx": 21095299
    }));
    round_trip::<generated::ConstrainedMetric>(serde_json::json!({
        "timestamp": 1623159380,
        "edge_status_2xx": 62616980,
        "edge_download_speed": { "0_250k": "0", "1M_2M": "0.09" }
    }));
    round_trip::<generated::ExampleDriven>(serde_json::json!({
        "actual_metric_name": 42,
        "another_valid_extra": true
    }));
    round_trip::<generated::ExamplesDriven>(serde_json::json!({
        "first_dynamic_name": true
    }));
    round_trip::<generated::Closed>(serde_json::json!({ "known": "value" }));
    round_trip::<generated::Typed>(serde_json::json!({
        "known": "declared",
        "extra": "typed"
    }));
}
"##,
    )
    .expect("scratch main");

    let output = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/dynamic-additional-properties-constraints-smoke"),
        )
        .output()
        .expect("cargo run");
    assert!(
        output.status.success(),
        "generated dynamic object round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
