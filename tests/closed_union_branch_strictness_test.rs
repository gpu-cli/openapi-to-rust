//! `#[serde(untagged)]` takes the first branch that deserializes, so a branch
//! that accepts more than its schema allows claims values belonging to a later
//! branch. A struct whose document states `additionalProperties: false` but
//! whose fields are all optional matches *any* object, wins the branch, and
//! drops the keys — `{"free": "shape"}` came back as `{}`.
//!
//! The negatives are the whole design. An omitted `additionalProperties`
//! leaves the object open in JSON Schema, and a closed struct outside any
//! union has no branch to lose, so neither is tightened: strict parsing there
//! would only make generated clients fail when a server adds a field.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::process::Command;

fn spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "closed branch", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            // States `additionalProperties: false` and is a union branch: the
            // one shape that must reject undeclared keys.
            "ClosedBranch": {
                "type": "object", "additionalProperties": false,
                "properties": { "id": { "type": "string" } }
            },
            // States nothing about extra keys, and is a union branch. JSON
            // Schema leaves it open, so it must stay tolerant.
            "OpenBranch": {
                "type": "object",
                "properties": { "name": { "type": "string" } }
            },
            // Closed, but never a union branch: nothing to lose, stays
            // tolerant so an added server field does not break the client.
            "ClosedStandalone": {
                "type": "object", "additionalProperties": false,
                "properties": { "id": { "type": "string" } }
            },
            "Holder": {
                "type": "object",
                "properties": {
                    "closed": { "anyOf": [
                        { "$ref": "#/components/schemas/ClosedBranch" },
                        { "type": "object" }
                    ] },
                    "open": { "anyOf": [
                        { "$ref": "#/components/schemas/OpenBranch" },
                        { "type": "string" }
                    ] },
                    "standalone": { "$ref": "#/components/schemas/ClosedStandalone" }
                }
            }
        } }
    })
}

fn generate(output_dir: std::path::PathBuf) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec()).expect("parse closed branch spec");
    let mut analysis = analyzer.analyze().expect("analyze closed branch spec");
    CodeGenerator::new(GeneratorConfig {
        output_dir,
        module_name: "closed_branch".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("generate closed branch models")
}

/// The attribute lands on exactly one of the three structs.
#[test]
fn only_a_stated_closed_union_branch_denies_unknown_fields() {
    let temp = tempfile::TempDir::new().expect("temporary output directory");
    let generated = generate(temp.path().join("generated"));

    let denied = |name: &str| {
        let anchor = format!("pub struct {name} ");
        let at = generated
            .find(&anchor)
            .unwrap_or_else(|| panic!("`{name}` must be generated:\n{generated}"));
        generated[..at]
            .rsplit("#[derive")
            .next()
            .unwrap_or_default()
            .contains("deny_unknown_fields")
    };

    assert!(
        denied("ClosedBranch"),
        "a stated `additionalProperties: false` in union-branch position must \
         reject undeclared keys:\n{generated}"
    );
    assert!(
        !denied("OpenBranch"),
        "an omitted `additionalProperties` leaves the object open in JSON \
         Schema and must stay tolerant:\n{generated}"
    );
    assert!(
        !denied("ClosedStandalone"),
        "a closed struct outside any union has no branch to lose and must stay \
         tolerant:\n{generated}"
    );
    assert!(
        !denied("Holder"),
        "the enclosing object declares nothing about extra keys:\n{generated}"
    );
}

#[test]
fn generated_unions_route_unknown_keys_to_the_branch_that_accepts_them() {
    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    let mut analyzer = SchemaAnalyzer::new(spec()).expect("parse closed branch spec");
    let mut analysis = analyzer.analyze().expect("analyze closed branch spec");
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: temp.path().join("src/generated"),
        module_name: "closed_branch".into(),
        enable_async_client: false,
        enable_sse_client: false,
        tracing_enabled: false,
        ..Default::default()
    });
    let result = generator
        .generate_all(&mut analysis)
        .expect("generate closed branch models");
    generator.write_files(&result).expect("write models");

    let dependency_fragment =
        std::fs::read_to_string(temp.path().join("src/generated/REQUIRED_DEPS.toml"))
            .expect("generated dependency fragment");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "closed-branch-smoke"
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
    fn an_undeclared_key_reaches_the_open_branch_and_survives() {
        let input = json!({"closed": {"free": "shape"}});
        let hydrated: generated::Holder =
            serde_json::from_value(input.clone()).expect("hydrate undeclared key");
        assert_eq!(
            serde_json::to_value(hydrated).unwrap(),
            input,
            "the key must reach the open branch instead of being dropped by the closed one"
        );
    }

    #[test]
    fn the_closed_branch_still_wins_the_shape_it_declares() {
        let hydrated: generated::Holder =
            serde_json::from_value(json!({"closed": {"id": "x"}})).expect("hydrate declared shape");
        assert!(matches!(
            hydrated.closed,
            Some(generated::HolderClosed::ClosedBranch(_))
        ));
    }

    #[test]
    fn a_tolerant_branch_still_absorbs_extra_keys_as_before() {
        // `OpenBranch` omits `additionalProperties`, so it keeps matching an
        // object carrying keys it does not declare.
        let hydrated: generated::Holder =
            serde_json::from_value(json!({"open": {"name": "n", "extra": 1}}))
                .expect("hydrate open branch");
        assert!(matches!(
            hydrated.open,
            Some(generated::HolderOpen::OpenBranch(_))
        ));
    }

    #[test]
    fn a_closed_standalone_struct_still_tolerates_a_new_server_field() {
        let hydrated: generated::ClosedStandalone =
            serde_json::from_value(json!({"id": "x", "addedLater": true}))
                .expect("a closed struct outside a union must not hard-fail");
        assert_eq!(hydrated.id.as_deref(), Some("x"));
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
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/closed-branch-smoke"),
        )
        .output()
        .expect("run generated branch tests");
    assert!(
        output.status.success(),
        "generated branch routing tests failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
