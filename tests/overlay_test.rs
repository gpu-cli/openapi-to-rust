#![allow(clippy::unwrap_used)]

use openapi_to_rust::overlay::{apply_overlay, effective_spec_json, preprocess_spec};
use serde_json::{Value, json};
use std::fs;

fn apply(document: Value, actions: Value) -> Result<Value, openapi_to_rust::GeneratorError> {
    apply_overlay(
        document,
        &json!({
            "overlay": "1.1.0", "info": {"title": "Test", "version": "1"}, "actions": actions
        }),
        "test.overlay.yaml",
    )
}

#[test]
fn ordered_update_copy_remove_and_recursive_merge() {
    let document = json!({"source": {"description": "copied", "tags": ["one"]},
        "dest": {"nested": {"keep": true}, "tags": ["zero"]}, "primitive": false});
    let result = apply(
        document,
        json!([
            {"target": "$.dest", "copy": "$.source"},
            {"target": "$.dest", "update": {"nested": {"new": 2}, "tags": ["two"]}},
            {"target": "$.primitive", "update": null},
            {"target": "$.source", "remove": true},
            {"target": "$.dest.tags", "update": "three"}
        ]),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"dest": {"nested": {"keep": true, "new": 2},
        "description": "copied", "tags": ["zero", "one", "two", "three"]}, "primitive": null})
    );
}

#[test]
fn unmatched_target_is_a_noop_including_missing_copy() {
    assert_eq!(
        apply(
            json!({}),
            json!([
                {"target": "$.absent", "copy": "$.missing"},
                {"target": "$.absent", "remove": true},
                {"target": "$.absent", "update": {"ignored": true}}
            ])
        )
        .unwrap(),
        json!({})
    );
}

#[test]
fn incompatible_kinds_mixed_targets_copy_cardinality_and_context() {
    for (document, action, expected) in [
        (
            json!({"obj": {"arr": []}}),
            json!({"target": "$.obj", "update": {"arr": "bad"}}),
            "incompatible value kinds at /obj/arr",
        ),
        (
            json!({"obj": {}}),
            json!({"target": "$.obj", "update": []}),
            "incompatible value kinds",
        ),
        (
            json!([{}, []]),
            json!({"target": "$[*]", "update": {}}),
            "all objects, all arrays, or all primitives",
        ),
        (
            json!({"to": {}, "from": [{}, {}]}),
            json!({"target": "$.to", "copy": "$.from[*]"}),
            "exactly one",
        ),
        (
            json!({"to": {}}),
            json!({"target": "$.to", "copy": "$.missing"}),
            "exactly one",
        ),
        (
            json!({"to": {}}),
            json!({"target": "$.to", "copy": "$.to", "update": {}}),
            "update and copy",
        ),
    ] {
        let error = apply(document, json!([action])).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(error.contains("test.overlay.yaml', action 1"), "{error}");
    }
}

#[test]
fn array_removals_use_original_indices_and_deduplicate() {
    let values: Vec<_> = (0..15).collect();
    let result = apply(
        json!({"values": values}),
        json!([
            {"target": "$.values[1,2,10,11,2]", "remove": true}
        ]),
    )
    .unwrap();
    assert_eq!(
        result["values"],
        json!([0, 3, 4, 5, 6, 7, 8, 9, 12, 13, 14])
    );
}

#[test]
fn overlap_copy_snapshot_and_repeated_targets_are_deterministic() {
    let result = apply(
        json!({"a": {"a": {"tags": []}, "tags": []}}),
        json!([
            {"target": "$..a", "update": {"tags": ["new"]}},
            {"target": "$.a.tags[0,0]", "update": 42},
            {"target": "$.a.tags", "copy": "$.a.tags"}
        ]),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"a": {"a": {"tags": ["new"]}, "tags": [42,42]}})
    );
    assert_eq!(
        apply(
            json!({"a": {"a": 1}, "b": 2}),
            json!([
                {"target": "$..a", "remove": true}
            ])
        )
        .unwrap(),
        json!({"b": 2})
    );
}

#[test]
fn remove_precedes_update_and_copy_and_root_removal_is_explicit() {
    assert_eq!(
        apply(
            json!({"a": 1}),
            json!([
                {"target": "$.a", "remove": true, "update": {}, "copy": "$.missing"}
            ])
        )
        .unwrap(),
        json!({})
    );
    let error = apply(json!({}), json!([{"target": "$", "remove": true}])).unwrap_err();
    assert!(error.to_string().contains("cannot remove the root"));
}

#[test]
fn document_validation_and_invalid_queries_include_context() {
    for overlay in [
        json!({"overlay": "1.0.0", "info": {"title": "x", "version": "1"}, "actions": [{}]}),
        json!({"overlay": "1.1.0", "info": {"title": "x"}, "actions": [{}]}),
        json!({"overlay": "1.1.0", "info": {"title": "x", "version": "1"}, "actions": []}),
    ] {
        assert!(
            apply_overlay(json!({}), &overlay, "broken.yaml")
                .unwrap_err()
                .to_string()
                .contains("broken.yaml")
        );
    }
    for action in [
        json!({"target": "$[broken", "update": null}),
        json!({"target": "$", "remove": "true"}),
    ] {
        assert!(
            apply(json!({}), json!([action]))
                .unwrap_err()
                .to_string()
                .contains("action 1")
        );
    }
}

#[test]
fn extensions_stay_distinct_and_overlays_apply_in_file_order() {
    let dir = tempfile::tempdir().unwrap();
    let extension = dir.path().join("extension.json");
    fs::write(
        &extension,
        r#"{"x-field": {"nested": true}, "x-remove": true}"#,
    )
    .unwrap();
    let first = dir.path().join("first.overlay.yaml");
    fs::write(
        &first,
        r#"overlay: 1.1.0
info: {title: First, version: '1'}
extends: https://example.invalid/declared-only.json
actions:
  - target: $['x-remove']
    remove: true
  - target: $['x-field']
    update: {first: true}
"#,
    )
    .unwrap();
    let second = dir.path().join("second.overlay.json");
    fs::write(
        &second,
        json!({"overlay":"1.1.0", "info":{"title":"Second","version":"1"},
        "actions":[{"target":"$['x-field'].first", "update":false}]})
        .to_string(),
    )
    .unwrap();
    let effective = preprocess_spec(
        json!({"x-field": "extension replaces this kind"}),
        &[extension],
        &[first, second],
    )
    .unwrap();
    assert_eq!(effective, json!({"x-field":{"nested":true,"first":false}}));
    let output = effective_spec_json(&effective).unwrap();
    assert!(output.ends_with('\n'));
    assert_eq!(output, effective_spec_json(&effective).unwrap());
}

#[test]
fn special_member_names_mutate_using_pointer_escaping() {
    let result = apply(
        json!({"slash/~": [0, 1, 2], "quote'": "before", "line\nfeed": false}),
        json!([
            {"target": "$['slash/~'][1]", "remove": true},
            {"target": "$[\"quote'\"]", "update": "after"},
            {"target": "$['line\\nfeed']", "update": true}
        ]),
    )
    .unwrap();
    assert_eq!(
        result,
        json!({"slash/~": [0,2], "quote'": "after", "line\nfeed": true})
    );
}

#[test]
fn config_overlay_paths_are_config_relative_but_artifacts_are_output_relative() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("spec.json"), "{}").unwrap();
    fs::write(dir.path().join("change.yaml"), "{}").unwrap();
    let path = dir.path().join("config.toml");
    fs::write(
        &path,
        r#"
[generator]
spec_path = "spec.json"
output_dir = "generated"
module_name = "api"
overlays = ["change.yaml"]
effective_spec = "effective.json"
bindings_metadata = "bindings.json"
[features]
"#,
    )
    .unwrap();
    let config = openapi_to_rust::ConfigFile::load(&path).unwrap();
    assert_eq!(
        config.generator.overlays,
        vec![dir.path().canonicalize().unwrap().join("change.yaml")]
    );
    assert_eq!(
        config.generator.effective_spec.as_deref(),
        Some(std::path::Path::new("effective.json"))
    );
    assert_eq!(
        config.generator.bindings_metadata.as_deref(),
        Some("bindings.json")
    );
    let serialized = toml::to_string(&config).unwrap();
    let roundtrip: openapi_to_rust::ConfigFile = toml::from_str(&serialized).unwrap();
    assert_eq!(roundtrip.generator.overlays, config.generator.overlays);
    assert_eq!(
        roundtrip.generator.effective_spec,
        config.generator.effective_spec
    );
}

#[test]
fn output_artifact_paths_reject_escapes_and_empty_names() {
    for invalid in [
        "",
        "/absolute.json",
        "../escape.json",
        "nested/../../escape.json",
        ".",
    ] {
        assert!(
            openapi_to_rust::overlay::validate_artifact_path(std::path::Path::new(invalid))
                .is_err()
        );
    }
    assert!(
        openapi_to_rust::overlay::validate_artifact_path(std::path::Path::new(
            "nested/effective.json"
        ))
        .is_ok()
    );
}

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_openapi-to-rust"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn direct_cli_preprocessing_repairs_version_and_checks_missing_effective_output() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("spec.json"),
        json!({"openapi":"0.0.0",
        "info":{"title":"Test", "version":"1"}, "paths":{}})
        .to_string(),
    )
    .unwrap();
    fs::write(
        dir.path().join("overlay.json"),
        json!({"overlay":"1.1.0",
        "info":{"title":"Repair", "version":"1"}, "actions":[
            {"target":"$.openapi", "update":"3.1.0"}
        ]})
        .to_string(),
    )
    .unwrap();
    let args = [
        "generate",
        "spec.json",
        "--overlay",
        "overlay.json",
        "--effective-spec",
        "effective.json",
        "--output-dir",
        "out",
        "--quiet",
    ];
    let preview = cli(dir.path(), &[&args[..], &["--dry-run"]].concat());
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(!dir.path().join("out").exists());
    let generated = cli(dir.path(), &args);
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let materialized = dir.path().join("out/effective.json");
    let effective: Value =
        serde_json::from_str(&fs::read_to_string(&materialized).unwrap()).unwrap();
    assert_eq!(effective["openapi"], "3.1.0");
    fs::remove_file(&materialized).unwrap();
    let checked = cli(dir.path(), &[&args[..], &["--check"]].concat());
    assert!(!checked.status.success());
    assert!(String::from_utf8_lossy(&checked.stderr).contains("missing: effective.json"));
    assert!(!materialized.exists());
}

#[test]
fn artifact_collision_errors_are_reported_before_any_writes() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("spec.json"),
        json!({"openapi":"3.1.0",
        "info":{"title":"Test", "version":"1"}, "paths":{}})
        .to_string(),
    )
    .unwrap();
    for path in [
        "types.rs",
        "REQUIRED_DEPS.toml",
        "types.rs/effective.json",
        "../escape.json",
    ] {
        let output = cli(
            dir.path(),
            &[
                "generate",
                "spec.json",
                "--types-only",
                "--effective-spec",
                path,
                "--output-dir",
                "out",
                "--quiet",
            ],
        );
        assert!(!output.status.success(), "{path}");
        assert!(
            !dir.path().join("out").exists(),
            "{path} wrote output before detecting collision"
        );
    }
}

#[test]
fn server_listing_uses_configured_overlays() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("spec.json"),
        json!({"openapi":"3.1.0",
        "info":{"title":"Test", "version":"1"}, "paths":{
            "/remove":{"get":{"operationId":"removeMe", "responses":{"204":{"description":"ok"}}}},
            "/keep":{"get":{"operationId":"keepMe", "responses":{"204":{"description":"ok"}}}}
        }})
        .to_string(),
    )
    .unwrap();
    fs::write(
        dir.path().join("overlay.json"),
        json!({"overlay":"1.1.0",
        "info":{"title":"Filter", "version":"1"}, "actions":[
            {"target":"$.paths['/remove']", "remove":true}
        ]})
        .to_string(),
    )
    .unwrap();
    fs::write(
        dir.path().join("config.toml"),
        r#"
[generator]
spec_path = "spec.json"
output_dir = "out"
module_name = "api"
overlays = ["overlay.json"]
[features]
"#,
    )
    .unwrap();
    let listing = cli(
        dir.path(),
        &["server", "list", "--config", "config.toml", "--json"],
    );
    assert!(
        listing.status.success(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    let body = String::from_utf8_lossy(&listing.stdout);
    assert!(body.contains("keepMe"));
    assert!(!body.contains("removeMe"));
    let explicit = cli(
        dir.path(),
        &[
            "server",
            "list",
            "--config",
            "config.toml",
            "--spec",
            "spec.json",
            "--json",
        ],
    );
    assert!(explicit.status.success());
    assert!(String::from_utf8_lossy(&explicit.stdout).contains("removeMe"));
}
