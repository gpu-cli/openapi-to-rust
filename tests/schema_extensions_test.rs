use openapi_to_rust::{SchemaAnalyzer, merge_schema_extensions};

fn write_overlay(directory: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn json_schema_extension_deep_merges_objects_and_arrays() {
    let directory = tempfile::tempdir().unwrap();
    let overlay = write_overlay(
        directory.path(),
        "overlay.json",
        r#"{
            "root": {
                "nested": { "from_json": true },
                "items": ["json"]
            }
        }"#,
    );
    let base = serde_json::json!({
        "root": {
            "kept": true,
            "nested": { "from_base": true },
            "items": ["base"]
        }
    });

    let merged = merge_schema_extensions(base, &[overlay]).unwrap();

    assert_eq!(merged["root"]["kept"], true);
    assert_eq!(merged["root"]["nested"]["from_base"], true);
    assert_eq!(merged["root"]["nested"]["from_json"], true);
    assert_eq!(merged["root"]["items"], serde_json::json!(["base", "json"]));
}

#[test]
fn yaml_schema_extension_deep_merges_objects_and_arrays() {
    let directory = tempfile::tempdir().unwrap();
    let overlay = write_overlay(
        directory.path(),
        "overlay.yaml",
        r#"root:
  nested:
    from_yaml: true
  items:
    - yaml
"#,
    );
    let base = serde_json::json!({
        "root": {
            "kept": true,
            "nested": { "from_base": true },
            "items": ["base"]
        }
    });

    let merged = merge_schema_extensions(base, &[overlay]).unwrap();

    assert_eq!(merged["root"]["kept"], true);
    assert_eq!(merged["root"]["nested"]["from_base"], true);
    assert_eq!(merged["root"]["nested"]["from_yaml"], true);
    assert_eq!(merged["root"]["items"], serde_json::json!(["base", "yaml"]));
}

#[test]
fn yaml_sse_overlay_marks_operation_as_streaming() {
    let directory = tempfile::tempdir().unwrap();
    let overlay = write_overlay(
        directory.path(),
        "sse-overlay.yml",
        r#"paths:
  /messages:
    post:
      responses:
        '200':
          content:
            text/event-stream:
              schema:
                type: string
"#,
    );
    let spec = serde_json::json!({
        "openapi": "3.0.3",
        "info": { "title": "overlay streaming", "version": "1" },
        "paths": {
            "/messages": {
                "post": {
                    "operationId": "createMessage",
                    "responses": {
                        "200": {
                            "description": "message",
                            "content": {
                                "application/json": {
                                    "schema": { "type": "object" }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new_with_extensions(spec, &[overlay]).unwrap();
    let analysis = analyzer.analyze().unwrap();
    let operation = analysis.operations.get("createMessage").unwrap();

    assert!(operation.supports_streaming);
}

#[test]
fn malformed_overlay_error_names_the_extension_path() {
    let directory = tempfile::tempdir().unwrap();
    let overlay = write_overlay(directory.path(), "broken.yaml", "paths: [\n");

    let error = merge_schema_extensions(serde_json::json!({}), &[&overlay]).unwrap_err();
    let message = error.to_string();

    assert!(
        message.contains(&overlay.display().to_string()),
        "{message}"
    );
    assert!(message.contains("as YAML"), "{message}");
}
