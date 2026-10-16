use openapi_to_rust::config::ClientSection;
use openapi_to_rust::{CodeGenerator, ConfigFile, DerivesConfig, GeneratorConfig, SchemaAnalyzer};
use quote::ToTokens;
use serde_json::json;
use std::{collections::BTreeMap, process::Command};

fn spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "additional derives", "version": "1" },
        "paths": {
            "/days": { "get": {
                "operationId": "listDays",
                "parameters": [{ "name": "day", "in": "query", "schema": {
                    "type": "string", "enum": ["Sunday", "Monday"]
                }}],
                "responses": { "200": { "description": "ok", "content": {
                    "application/json": { "schema": { "$ref": "#/components/schemas/Packet" } }
                }}}
            }},
            "/coordinates": { "get": {
                "operationId": "getCoordinates",
                "responses": { "200": { "description": "ok", "content": {
                    "application/json": { "schema": { "$ref": "#/components/schemas/Coordinates" } }
                }}}
            }}
        },
        "components": { "schemas": {
            "day-of-week": { "type": "string", "enum": ["Sunday", "Monday", "Tuesday"] },
            "OpenLabel": { "anyOf": [
                { "type": "string", "enum": ["known"] }, { "type": "string" }
            ]},
            "Packet": { "type": "object", "properties": {
                "day": { "$ref": "#/components/schemas/day-of-week" },
                "status": { "type": "string", "enum": ["pending", "ready"] }
            }},
            "Coordinates": { "type": "object", "additionalProperties": false,
                "required": ["x"], "properties": { "x": { "type": "integer" } } },
            "CountOrText": { "anyOf": [{ "type": "integer" }, { "type": "string" }] },
            "MixedValue": { "anyOf": [{ "type": "number" }, { "type": "string" }] },
            "ExclusiveValue": { "oneOf": [{ "type": "integer" }, { "type": "string" }] },
            "ObjectUnion": { "anyOf": [
                { "$ref": "#/components/schemas/Coordinates" },
                { "$ref": "#/components/schemas/Packet" }
            ]},
            "Counter": { "type": "integer" }
        }}
    })
}

fn config(derives: DerivesConfig) -> GeneratorConfig {
    GeneratorConfig {
        enable_async_client: false,
        enable_sse_client: false,
        derives,
        ..Default::default()
    }
}

fn types(derives: DerivesConfig) -> openapi_to_rust::Result<String> {
    let mut analysis = SchemaAnalyzer::new(spec())?.analyze()?;
    CodeGenerator::new(config(derives)).generate(&mut analysis)
}

fn per_type(name: &str, values: &[&str]) -> DerivesConfig {
    DerivesConfig {
        types: BTreeMap::from([(
            name.into(),
            values.iter().map(|value| (*value).into()).collect(),
        )]),
        ..Default::default()
    }
}

fn attributes(source: &str, name: &str) -> String {
    syn::parse_file(source)
        .unwrap()
        .items
        .into_iter()
        .find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == name => Some(item.attrs),
            syn::Item::Struct(item) if item.ident == name => Some(item.attrs),
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing generated type {name}"))
        .iter()
        .map(|attr| attr.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn config_loads_round_trips_and_reaches_library_generation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("spec.json"), spec().to_string()).unwrap();
    let path = dir.path().join("generator.toml");
    std::fs::write(
        &path,
        r#"
[generator]
spec_path = "spec.json"
output_dir = "generated"
module_name = "api"
[generator.derives]
string_enums = ["PartialOrd", "Ord"]
[generator.derives.types]
DayOfWeek = ["Hash"]
[features]
enable_async_client = false
"#,
    )
    .unwrap();
    let loaded = ConfigFile::load(&path).unwrap();
    let serialized = toml::to_string(&loaded).unwrap();
    let reparsed: ConfigFile = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.generator.derives, loaded.generator.derives);
    let config = loaded.into_generator_config();
    assert_eq!(config.derives.types["DayOfWeek"], ["Hash"]);
    let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
    let generated = CodeGenerator::new(config).generate(&mut analysis).unwrap();
    assert!(attributes(&generated, "DayOfWeek").contains("PartialOrd , Ord , Hash"));
    assert_eq!(DerivesConfig::default(), GeneratorConfig::default().derives);
}

#[cfg(feature = "cli")]
#[test]
fn cli_generation_and_stale_checks_honor_derive_settings() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("spec.json"), spec().to_string()).unwrap();
    let path = dir.path().join("generator.toml");
    let initial = r#"
[generator]
spec_path = "spec.json"
output_dir = "generated"
module_name = "api"
[generator.derives]
string_enums = ["PartialOrd", "Ord"]
[features]
enable_async_client = false
"#;
    std::fs::write(&path, initial).unwrap();
    let generate = |extra: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_openapi-to-rust"))
            .args(["generate", "--config"])
            .arg(&path)
            .args(extra)
            .output()
            .unwrap()
    };
    let result = generate(&[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let output = dir.path().join("generated/types.rs");
    let before = std::fs::read_to_string(&output).unwrap();
    assert!(attributes(&before, "DayOfWeek").contains("PartialOrd , Ord"));
    assert!(generate(&["--check"]).status.success());
    std::fs::write(
        &path,
        initial.replace(
            "[\"PartialOrd\", \"Ord\"]",
            "[\"PartialOrd\", \"Ord\", \"Hash\"]",
        ),
    )
    .unwrap();
    assert!(!generate(&["--check"]).status.success());
    assert!(generate(&["--dry-run"]).status.success());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), before);
    assert!(generate(&[]).status.success());
    assert!(generate(&["--check"]).status.success());
    assert!(attributes(&std::fs::read_to_string(output).unwrap(), "DayOfWeek").contains("Hash"));
}

#[test]
fn malformed_macro_paths_fail_for_toml_and_library_consumers() {
    for invalid in ["", "Ord, Hash", "Ord<u8>", "Ord()", "#[derive(Ord)]"] {
        let derives = DerivesConfig {
            string_enums: vec![invalid.into()],
            ..Default::default()
        };
        let error = types(derives).unwrap_err().to_string();
        assert!(error.contains("Invalid derive macro path"), "{error}");
        let toml = format!(
            r#"
[generator]
spec_path = "spec.json"
output_dir = "generated"
module_name = "api"
[generator.derives]
string_enums = [{invalid:?}]
[features]
"#
        );
        assert!(toml::from_str::<ConfigFile>(&toml).is_err(), "{invalid}");
    }
}

#[test]
fn group_applies_to_string_enums_and_leaves_unions_structs_and_helpers_alone() {
    let source = types(DerivesConfig {
        string_enums: vec!["PartialOrd".into(), "Ord".into(), "Hash".into()],
        ..Default::default()
    })
    .unwrap();
    for name in ["DayOfWeek", "OpenLabel", "PacketStatus"] {
        assert!(
            attributes(&source, name).contains("PartialOrd , Ord , Hash"),
            "{name}"
        );
    }
    for name in [
        "Packet",
        "Coordinates",
        "CountOrText",
        "MixedValue",
        "ExclusiveValue",
    ] {
        assert!(!attributes(&source, name).contains("Ord"), "{name}");
    }
}

#[test]
fn per_type_uses_emitted_names_and_supports_structs_and_payload_enums() {
    let source = types(per_type(
        "Coordinates",
        &["PartialEq", "Eq", "PartialOrd", "Ord", "Hash"],
    ))
    .unwrap();
    assert!(
        attributes(&source, "Coordinates").contains("PartialEq , Eq , PartialOrd , Ord , Hash")
    );
    assert!(!attributes(&source, "DayOfWeek").contains("Hash"));
    let source = types(per_type("DayOfWeek", &["Ord", "PartialOrd"])).unwrap();
    assert!(attributes(&source, "DayOfWeek").contains("Ord , PartialOrd"));
    let source = types(per_type(
        "MixedValue",
        &["PartialEq", "Eq", "PartialOrd", "Ord"],
    ))
    .unwrap();
    assert!(attributes(&source, "MixedValue").contains("PartialEq , Eq , PartialOrd , Ord"));
    // The generator must emit the request, leaving payload bounds to rustc.
}

#[test]
fn unknown_alias_and_ambiguous_targets_are_diagnosed() {
    for (name, expected) in [
        ("Missing", "unknown generated type"),
        ("day-of-week", "emitted Rust type name"),
        ("Counter", "type alias"),
    ] {
        let error = types(per_type(name, &["Hash"])).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
    }
    let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
    let parameter_name = analysis.operations["listDays"].parameters[0]
        .rust_type
        .clone();
    let mut duplicate = analysis.schemas["Coordinates"].clone();
    duplicate.name = parameter_name.clone();
    analysis.schemas.insert(parameter_name.clone(), duplicate);
    let error = CodeGenerator::new(config(per_type(&parameter_name, &["Hash"])))
        .generate_all(&mut analysis)
        .unwrap_err()
        .to_string();
    assert!(error.contains("ambiguous"), "{error}");
}

#[test]
fn existing_derives_are_deduplicated_and_specta_remains_conditional() {
    let baseline = types(DerivesConfig::default()).unwrap();
    let repeated = types(DerivesConfig {
        string_enums: vec![
            "Debug".into(),
            "::core::fmt::Debug".into(),
            "std::cmp::Eq".into(),
            "Clone".into(),
        ],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(repeated, baseline);
    let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
    let mut cfg = config(per_type(
        "DayOfWeek",
        &["specta::Type", "Hash", "::std::hash::Hash"],
    ));
    cfg.enable_specta = true;
    let source = CodeGenerator::new(cfg).generate(&mut analysis).unwrap();
    let attrs = attributes(&source, "DayOfWeek");
    assert_eq!(attrs.matches("specta :: Type").count(), 1);
    assert_eq!(attrs.matches("Hash").count(), 1);
    assert!(attrs.contains("cfg_attr"));
}

#[test]
fn custom_macro_paths_with_the_same_name_remain_distinct() {
    let source = types(per_type(
        "DayOfWeek",
        &["a::Marker", "b::Marker", "a::Marker"],
    ))
    .unwrap();
    let attrs = attributes(&source, "DayOfWeek");
    assert_eq!(attrs.matches("a :: Marker").count(), 1);
    assert_eq!(attrs.matches("b :: Marker").count(), 1);
}

#[test]
fn manual_wire_and_display_implementations_are_protected() {
    for (name, derive) in [
        ("OpenLabel", "Serialize"),
        ("OpenLabel", "serde::Deserialize"),
        ("DayOfWeek", "std::fmt::Display"),
        ("ObjectUnion", "serde::Serialize"),
    ] {
        let error = types(per_type(name, &[derive])).unwrap_err().to_string();
        assert!(
            error.contains("already implements that trait manually"),
            "{error}"
        );
    }
}

#[test]
fn valid_targets_removed_by_selection_or_pruning_are_ignored() {
    let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
    let mut cfg = config(per_type("Coordinates", &["Hash"]));
    cfg.enable_async_client = true;
    cfg.client = Some(ClientSection {
        operations: vec!["listDays".into()],
        prune_models: true,
    });
    let generated = CodeGenerator::new(cfg).generate_all(&mut analysis).unwrap();
    assert!(generated.pruned_schemas > 0);
    assert!(!analysis.schemas.contains_key("Coordinates"));
}

#[test]
fn parameter_derives_and_binding_metadata_share_the_emitted_ast() {
    let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
    let name = analysis.operations["listDays"].parameters[0]
        .rust_type
        .clone();
    let mut cfg = config(per_type(&name, &["Hash"]));
    cfg.enable_async_client = true;
    cfg.derives.string_enums = vec!["PartialOrd".into(), "Ord".into()];
    let generator = CodeGenerator::new(cfg);
    let standalone = generator.generate_http_client(&analysis).unwrap();
    assert!(attributes(&standalone, &name).contains("PartialOrd , Ord , Hash"));
    let generated = generator.generate_all_with_bindings(&mut analysis).unwrap();
    let symbol = generated
        .bindings
        .symbols
        .iter()
        .find(|symbol| symbol.path == format!("client::{name}"))
        .unwrap();
    assert!(
        symbol
            .attributes
            .iter()
            .any(|attr| attr.contains("PartialOrd"))
    );
    let day = generated
        .bindings
        .symbols
        .iter()
        .find(|symbol| symbol.path == "types::DayOfWeek")
        .unwrap();
    assert!(day.attributes.iter().any(|attr| attr.contains("Ord")));
    assert!(!day.attributes.iter().any(|attr| attr.contains("Hash")));
}

#[test]
fn generated_derives_compile_and_preserve_map_order_and_wire_round_trips() {
    let temp = tempfile::tempdir().unwrap();
    let mut analysis = SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap();
    let parameter_name = analysis.operations["listDays"].parameters[0]
        .rust_type
        .clone();
    let mut derives = per_type(
        "Coordinates",
        &[
            "PartialEq",
            "Eq",
            "PartialOrd",
            "Ord",
            "Hash",
            "custom_derives::Marker",
        ],
    );
    derives.types.insert(
        "CountOrText".into(),
        ["PartialEq", "Eq", "PartialOrd", "Ord", "Hash"]
            .map(String::from)
            .into(),
    );
    derives.string_enums = ["PartialOrd", "Ord", "Hash", "custom_derives::Marker"]
        .map(String::from)
        .into();
    let generator = CodeGenerator::new(config(derives.clone()));
    let source = generator.generate(&mut analysis).unwrap();
    let client = generator.generate_http_client(&analysis).unwrap();
    let parameter = syn::parse_file(&client)
        .unwrap()
        .items
        .into_iter()
        .find(|item| matches!(item, syn::Item::Enum(item) if item.ident == parameter_name))
        .unwrap()
        .to_token_stream()
        .to_string();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::create_dir_all(temp.path().join("custom-derives/src")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"
[package]
name = "generated-derive-smoke"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
custom-derives = { path = "custom-derives" }
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("custom-derives/Cargo.toml"),
        r#"
[package]
name = "custom-derives"
version = "0.0.0"
edition = "2024"
[lib]
proc-macro = true
"#,
    )
    .unwrap();
    std::fs::write(
        temp.path().join("custom-derives/src/lib.rs"),
        r#"
extern crate proc_macro;
#[proc_macro_derive(Marker)]
pub fn marker(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let mut tokens = input.into_iter();
    while let Some(token) = tokens.next() {
        if matches!(token.to_string().as_str(), "struct" | "enum") {
            let name = tokens.next().unwrap();
            return format!("impl crate::Marker for {name} {{}}").parse().unwrap();
        }
    }
    panic!("missing type");
}
"#,
    )
    .unwrap();
    std::fs::write(temp.path().join("src/types.rs"), source).unwrap();
    std::fs::write(temp.path().join("src/parameter.rs"), parameter).unwrap();
    std::fs::write(temp.path().join("src/main.rs"), r#"
#![allow(dead_code)]
mod types;
mod parameter;
pub trait Marker {}
fn marker<T: Marker>() {}
fn ordered_key<T: Ord + std::hash::Hash>() {}
fn main() {
    use types::*;
    use std::collections::{BTreeMap, HashMap};
    let days = BTreeMap::from([(DayOfWeek::Monday, 2), (DayOfWeek::Sunday, 1)]);
    assert_eq!(days.keys().map(DayOfWeek::as_str).collect::<Vec<_>>(), ["Sunday", "Monday"]);
    assert_eq!(HashMap::from([(DayOfWeek::Sunday, 1)]).get(&DayOfWeek::Sunday), Some(&1));
    assert_eq!(serde_json::to_string(&DayOfWeek::Sunday).unwrap(), "\"Sunday\"");
    let labels = BTreeMap::from([(OpenLabel::Custom("z".into()), 3), (OpenLabel::Known, 1), (OpenLabel::Custom("a".into()), 2)]);
    assert_eq!(labels.keys().map(OpenLabel::as_str).collect::<Vec<_>>(), ["known", "a", "z"]);
    for label in labels.keys() {
        let wire = serde_json::to_string(label).unwrap();
        assert_eq!(&serde_json::from_str::<OpenLabel>(&wire).unwrap(), label);
    }
    ordered_key::<Coordinates>();
    ordered_key::<CountOrText>();
    ordered_key::<PacketStatus>();
    ordered_key::<parameter::__PARAMETER__>();
    marker::<DayOfWeek>();
    marker::<Coordinates>();
    marker::<parameter::__PARAMETER__>();
    let day = parameter::__PARAMETER__::Monday;
    assert_eq!(serde_json::to_string(&day).unwrap(), "\"Monday\"");
    assert!(parameter::__PARAMETER__::Sunday < day);
}
"#.replace("__PARAMETER__", &parameter_name)).unwrap();
    let run = |action: &str| {
        Command::new("cargo")
            .args([action, "--quiet"])
            .current_dir(temp.path())
            .env(
                "CARGO_TARGET_DIR",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/derive-smoke"),
            )
            .output()
            .unwrap()
    };
    let output = run("run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    derives.types.insert(
        "MixedValue".into(),
        ["PartialEq", "Eq", "PartialOrd", "Ord"]
            .map(String::from)
            .into(),
    );
    let source = CodeGenerator::new(config(derives))
        .generate(&mut analysis)
        .unwrap();
    std::fs::write(temp.path().join("src/types.rs"), source).unwrap();
    let output = run("check");
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(errors.contains("f64: Ord"), "{errors}");
}
