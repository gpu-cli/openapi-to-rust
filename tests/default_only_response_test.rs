//! An operation that declares no 2xx response selects every successful status,
//! so its success guard is `status.is_success()` itself. The branch that
//! rejects successful statuses the return type didn't select must not follow
//! it: it can't be reached, and `if c {} else if c {}` is
//! `clippy::ifs_same_cond`, which is deny-by-default.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn generate_client(spec: serde_json::Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer");
    let analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(GeneratorConfig::default());
    generator
        .generate_http_client(&analysis)
        .expect("client generation")
}

fn error() -> serde_json::Value {
    json!({
        "description": "error",
        "content": { "application/json": { "schema": {
            "type": "object",
            "properties": { "message": { "type": "string" } }
        }}}
    })
}

/// The body of the generated method `name`, up to the next method.
fn method<'a>(client: &'a str, name: &str) -> &'a str {
    let start = client
        .find(&format!("pub async fn {name}("))
        .unwrap_or_else(|| panic!("no method {name}"));
    let rest = &client[start + 1..];
    let end = rest.find("pub async fn ").map_or(rest.len(), |end| end + 1);
    &client[start..start + end]
}

#[test]
fn default_only_response_has_no_unreachable_success_branch() {
    let client = generate_client(json!({
        "openapi": "3.0.3",
        "info": { "title": "default only", "version": "1" },
        "paths": {
            "/default-only": { "delete": {
                "operationId": "defaultOnly",
                "responses": { "default": error() }
            }},
            "/declared": { "delete": {
                "operationId": "declared",
                "responses": { "204": { "description": "deleted" }, "default": error() }
            }}
        }
    }));

    let body = method(&client, "default_only");
    assert_eq!(
        body.matches("is_success()").count(),
        1,
        "default_only tests `status.is_success()` once, as its success guard:\n{body}"
    );
    assert!(
        !body.contains("unexpected successful status"),
        "default_only has no successful status to reject:\n{body}"
    );

    // A declared 2xx status still rejects the others.
    let body = method(&client, "declared");
    assert!(
        body.contains("status_code == 204"),
        "declared selects 204:\n{body}"
    );
    assert!(
        body.contains("else if status.is_success()")
            && body.contains("unexpected successful status"),
        "declared rejects the successful statuses it didn't select:\n{body}"
    );
}
