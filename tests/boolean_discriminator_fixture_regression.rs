use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn load_fixture(relative_path: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_str(&source)
            .unwrap_or_else(|error| panic!("failed to parse {} as JSON: {error}", path.display())),
        Some("yaml" | "yml") => serde_yaml::from_str(&source)
            .unwrap_or_else(|error| panic!("failed to parse {} as YAML: {error}", path.display())),
        extension => panic!(
            "unsupported fixture extension {extension:?}: {}",
            path.display()
        ),
    }
}

fn component_schema<'a>(fixture: &'a Value, name: &str) -> &'a Value {
    fixture
        .pointer(&format!("/components/schemas/{name}"))
        .unwrap_or_else(|| panic!("missing #/components/schemas/{name}"))
}

fn assert_exact_any_of_refs(schema: &Value, expected: &[&str]) {
    let actual = schema["anyOf"]
        .as_array()
        .expect("anyOf should be an array")
        .iter()
        .map(|branch| {
            branch["$ref"]
                .as_str()
                .expect("every anyOf branch should be a reference")
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn assert_boolean_literal(property: &Value, expected: bool) {
    assert_eq!(property["type"], json!("boolean"));
    assert_eq!(property["const"], json!(expected));
    assert_eq!(property["enum"], json!([expected]));
    assert_eq!(property["examples"], json!([expected]));
}

#[test]
fn gcore_slurm_union_uses_boolean_branches_without_a_string_discriminator() {
    let fixture = load_fixture("specs/gcore.yaml");
    let union = component_schema(&fixture, "K8sClusterSlurmAddonV2Serializers");

    assert_eq!(union.get("discriminator"), None);
    assert_exact_any_of_refs(
        union,
        &[
            "#/components/schemas/K8sClusterSlurmAddonEnableV2Serializer",
            "#/components/schemas/K8sClusterSlurmAddonDisableV2Serializer",
        ],
    );

    let enabled = component_schema(&fixture, "K8sClusterSlurmAddonEnableV2Serializer");
    let disabled = component_schema(&fixture, "K8sClusterSlurmAddonDisableV2Serializer");
    assert_boolean_literal(&enabled["properties"]["enabled"], true);
    assert_boolean_literal(&disabled["properties"]["enabled"], false);
}

#[test]
fn cloudflare_hyperdrive_union_keeps_its_boolean_typed_branches() {
    let fixture = load_fixture("specs/cloudflare.yaml");
    let union = component_schema(&fixture, "hyperdrive_hyperdrive-caching");

    assert_eq!(union.get("discriminator"), None);
    assert_exact_any_of_refs(
        union,
        &[
            "#/components/schemas/hyperdrive_hyperdrive-caching-disabled",
            "#/components/schemas/hyperdrive_hyperdrive-caching-enabled",
        ],
    );

    let common = component_schema(&fixture, "hyperdrive_hyperdrive-caching-common");
    let enabled = component_schema(&fixture, "hyperdrive_hyperdrive-caching-enabled");
    assert_eq!(common["properties"]["disabled"]["type"], "boolean");
    assert_eq!(enabled["properties"]["disabled"]["type"], "boolean");
}

#[test]
fn boolean_branch_union_generates_untagged_without_manual_string_dispatch() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "boolean caching", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Caching": {
                    "type": "object",
                    "anyOf": [
                        { "$ref": "#/components/schemas/CachingDisabled" },
                        { "$ref": "#/components/schemas/CachingEnabled" }
                    ]
                },
                "CachingDisabled": {
                    "type": "object",
                    "properties": { "disabled": { "type": "boolean" } }
                },
                "CachingEnabled": {
                    "type": "object",
                    "properties": {
                        "disabled": { "type": "boolean" },
                        "max_age": { "type": "integer" }
                    }
                }
            }
        }
    });

    let mut analysis = SchemaAnalyzer::new(spec)
        .expect("minimal spec should parse")
        .analyze()
        .expect("minimal spec should analyze");
    assert!(matches!(
        analysis.schemas["Caching"].schema_type,
        SchemaType::Union { .. }
    ));

    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("discriminator-free boolean union should generate valid Rust");
    assert!(generated.contains("#[serde(untagged)]"));
    assert!(generated.contains("pub enum Caching"));
    assert!(!generated.contains("missing string discriminator"));
    assert!(!generated.contains("Value::String(\"false\""));
    assert!(!generated.contains("\"false\" =>"));
    assert!(!generated.contains("r#false"));
}
