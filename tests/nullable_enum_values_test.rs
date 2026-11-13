use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn spec(version: &str) -> Value {
    let mut status = json!({"enum": ["active", null, "archived"]});
    match version {
        "3.0.3" => {
            status["type"] = json!("string");
            status["nullable"] = json!(true);
        }
        "3.1.0" => status["type"] = json!(["string", "null"]),
        "typeless" => {}
        _ => unreachable!(),
    }
    let mut null_only = status.clone();
    null_only["enum"] = json!([null]);
    json!({
        "openapi": if version == "typeless" { "3.1.0" } else { version },
        "info": {"title": "nullable enums", "version": "1"},
        "paths": {},
        "components": {"schemas": {
            "State": status,
            "OnlyNull": null_only,
            "ImpossibleStatus": {"type": "string", "enum": [null]},
            "Item": {
                "type": "object",
                "required": ["status", "referenced", "items", "literal", "null_only", "null_ref", "null_items"],
                "properties": {
                    "status": status,
                    "optional": status,
                    "referenced": {"$ref": "#/components/schemas/State"},
                    "items": {"type": "array", "items": status},
                    "null_only": null_only,
                    "null_ref": {"$ref": "#/components/schemas/OnlyNull"},
                    "null_items": {"type": "array", "items": null_only},
                    "literal": {"type": "string", "nullable": true, "enum": ["null", null, "other"]}
                }
            }
        }}
    })
}

#[test]
fn nullable_enums_round_trip_null_and_reject_the_unlisted_string() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("src")).unwrap();
    let mut modules = String::new();
    for (index, version) in ["3.0.3", "3.1.0", "typeless"].iter().enumerate() {
        let mut analysis = SchemaAnalyzer::new(spec(version))
            .unwrap()
            .analyze()
            .unwrap();
        let mut generated = CodeGenerator::new(GeneratorConfig::default())
            .generate(&mut analysis)
            .unwrap();
        generated.push_str(r#"
#[cfg(test)]
mod regression {
    use super::*;
    #[test]
    fn null_and_string_null_are_distinct() {
        // An explicit non-nullable string type intersected with enum: [null]
        // admits no values. It must compile without inventing a string value.
        for wire in [serde_json::json!(null), serde_json::json!("null"), serde_json::json!("active")] {
            assert!(serde_json::from_value::<ImpossibleStatus>(wire).is_err());
        }
        for status in [serde_json::json!(null), serde_json::json!("active"), serde_json::json!("archived")] {
            let wire = serde_json::json!({
                "status": status, "referenced": status,
                "items": [null, "active", "archived"], "literal": "null",
                "null_only": null, "null_ref": null, "null_items": [null]
            });
            let item: Item = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(item).unwrap(), wire);
            for field in ["status", "referenced", "optional", "null_only", "null_ref"] {
                let mut invalid = wire.clone();
                invalid[field] = serde_json::json!("null");
                assert!(serde_json::from_value::<Item>(invalid).is_err(), "{field}");
            }
            let mut invalid = wire;
            invalid["items"] = serde_json::json!(["null"]);
            assert!(serde_json::from_value::<Item>(invalid).is_err());
        }
    }
}
"#);
        let module = format!("case_{index}");
        fs::write(temp.path().join(format!("src/{module}.rs")), generated).unwrap();
        modules.push_str(&format!("pub mod {module};\n"));
    }
    fs::write(temp.path().join("src/lib.rs"), modules).unwrap();
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"
[package]
name = "nullable-enum-values-smoke"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .unwrap();
    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/nullable-enum-values-smoke"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn null_enum_metadata_keeps_original_positions() {
    for null_index in 0..3 {
        let mut values = vec![json!("active"), json!("archived")];
        let mut names = vec!["Current", "Old"];
        let mut descriptions = vec!["Current state", "Archived state"];
        values.insert(null_index, Value::Null);
        names.insert(null_index, "Missing");
        descriptions.insert(null_index, "No state");
        let mut input = spec("3.1.0");
        let state = &mut input["components"]["schemas"]["State"];
        state["enum"] = json!(values);
        state["x-enum-varnames"] = json!(names);
        state["x-enum-descriptions"] = json!(descriptions);
        let analysis = SchemaAnalyzer::new(input).unwrap().analyze().unwrap();
        let ext = &analysis.enum_extensions["State"];
        assert_eq!(ext.varnames, ["Current", "Old"]);
        assert_eq!(ext.descriptions, ["Current state", "Archived state"]);
    }
    // Metadata must describe the original array, not just its non-null subset.
    let mut input = spec("3.1.0");
    input["components"]["schemas"]["State"]["x-enum-varnames"] = json!(["Wrong", "Names"]);
    let analysis = SchemaAnalyzer::new(input).unwrap().analyze().unwrap();
    assert!(!analysis.enum_extensions.contains_key("State"));
}
