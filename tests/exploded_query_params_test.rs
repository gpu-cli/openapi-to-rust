//! Regression tests for https://github.com/gpu-cli/openapi-to-rust/issues/27
//!
//! Query parameters with object schemas and form-explode semantics previously
//! mapped to `Option<impl AsRef<str>>` and were sent as a single opaque
//! `name=<string>` pair. Per OAS 3.x (style=form + explode=true are the
//! defaults for `in: query`) and RFC 6570 form-explosion, each object property
//! must become its own query pair: `?color=red&size=big` — the parameter name
//! itself never appears in the query string.
//!
//! Now: the analyzer types those parameters as a struct (synthesized for
//! inline schemas, resolved for $refs) and the client emits `req.query(&v)`,
//! which reqwest/serde_urlencoded serializes property-by-property. Styles we
//! don't generate yet (deepObject, form with explode=false) keep the string
//! fallback.
//!
//! Method-body assertions use the raw token-stream spacing
//! (`filter : Option < FindWidgetsFilter >`), matching
//! `parameter_integer_format_test.rs`.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, analysis::SchemaAnalyzer};
use serde_json::{Value, json};
use std::path::PathBuf;

fn config() -> GeneratorConfig {
    GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    }
}

/// Token-spaced client method text for assertions on signatures/bodies.
fn generate_methods(spec: Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(config());
    generator.generate_operation_methods(&analysis).to_string()
}

/// Full pretty-printed types output, for asserting emitted model structs.
fn generate_types(spec: Value) -> String {
    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let mut analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(config());
    generator.generate(&mut analysis).expect("generation")
}

fn spec_with_filter_param(param: Value) -> Value {
    json!({
        "openapi": "3.0.3",
        "info": {"title": "Exploded Query Param Repro", "version": "1.0.0"},
        "paths": {
            "/widgets": {
                "get": {
                    "operationId": "findWidgets",
                    "parameters": [param],
                    "responses": {"200": {"description": "OK"}}
                }
            }
        }
    })
}

fn inline_object_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "color": {"type": "string"},
            "size": {"type": "integer"}
        }
    })
}

#[test]
fn inline_object_with_explicit_form_explode_generates_struct_param() {
    let spec = spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "style": "form",
        "explode": true,
        "schema": inline_object_schema()
    }));
    let code = generate_methods(spec.clone());

    assert!(
        code.contains("filter : Option < FindWidgetsFilter >"),
        "exploded object param should be typed as the synthesized struct; got:\n{code}"
    );
    assert!(
        code.contains("req = req . query (& v)"),
        "optional exploded param must serialize via req.query(&v); got:\n{code}"
    );
    assert!(
        !code.contains("(\"filter\" , v . as_ref () . to_string ())"),
        "exploded param must not be sent as a single `filter=<string>` pair; got:\n{code}"
    );

    let types = generate_types(spec);
    assert!(
        types.contains("pub struct FindWidgetsFilter"),
        "synthesized struct for the inline object schema must be emitted; got:\n{types}"
    );
    assert!(
        types.contains("pub color: Option<String>"),
        "struct must carry the object's properties; got:\n{types}"
    );
}

#[test]
fn object_query_param_defaults_to_form_explode() {
    // OAS defaults for `in: query` are style=form, explode=true — an object
    // param with neither specified is already exploded per spec.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "schema": inline_object_schema()
    })));

    assert!(
        code.contains("filter : Option < FindWidgetsFilter >"),
        "object query param with default style/explode should be exploded; got:\n{code}"
    );
    assert!(
        code.contains("req = req . query (& v)"),
        "default-exploded param must serialize via req.query(&v); got:\n{code}"
    );
}

#[test]
fn required_exploded_object_param_is_not_option() {
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "required": true,
        "schema": inline_object_schema()
    })));

    assert!(
        code.contains("filter : FindWidgetsFilter"),
        "required exploded param should be a bare struct arg; got:\n{code}"
    );
    assert!(
        code.contains("req = req . query (& filter)"),
        "required exploded param must serialize unconditionally; got:\n{code}"
    );
}

#[test]
fn ref_object_query_param_uses_referenced_struct() {
    let mut spec = spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "explode": true,
        "schema": {"$ref": "#/components/schemas/WidgetFilter"}
    }));
    spec["components"] = json!({
        "schemas": {
            "WidgetFilter": {
                "type": "object",
                "properties": {"color": {"type": "string"}}
            }
        }
    });
    let code = generate_methods(spec);

    assert!(
        code.contains("filter : Option < WidgetFilter >"),
        "$ref exploded object param should use the referenced struct; got:\n{code}"
    );
    assert!(
        code.contains("req = req . query (& v)"),
        "$ref exploded param must serialize via req.query(&v); got:\n{code}"
    );
}

#[test]
fn explode_false_keeps_string_fallback() {
    // form + explode=false (`?filter=color,red,size,big`) isn't generated
    // yet — the parameter must keep the pre-#27 string passthrough.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "explode": false,
        "schema": inline_object_schema()
    })));

    assert!(
        code.contains("filter : Option < impl AsRef < str > >"),
        "explode=false object param should keep the string fallback; got:\n{code}"
    );
    assert!(
        !code.contains("FindWidgetsFilter"),
        "no struct should be synthesized for explode=false; got:\n{code}"
    );
}

#[test]
fn deep_object_style_keeps_string_fallback() {
    // deepObject (`?filter[color]=red`) isn't generated yet — string fallback.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "style": "deepObject",
        "explode": true,
        "schema": inline_object_schema()
    })));

    assert!(
        code.contains("filter : Option < impl AsRef < str > >"),
        "deepObject param should keep the string fallback; got:\n{code}"
    );
}

/// Runtime proof of the mechanism the generated code relies on: reqwest
/// serializes a struct passed to `.query(&v)` through serde_urlencoded,
/// yielding one `key=value` pair per set property and omitting `None`s —
/// i.e. exactly RFC 6570 form-explosion for flat objects.
#[test]
fn reqwest_query_serializes_struct_as_exploded_pairs() {
    #[derive(serde::Serialize)]
    struct FindWidgetsFilter {
        #[serde(skip_serializing_if = "Option::is_none")]
        color: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        size: Option<i64>,
    }

    let filter = FindWidgetsFilter {
        color: Some("red".to_string()),
        size: Some(5),
    };
    let req = reqwest::Client::new()
        .get("https://api.example.com/widgets")
        .query(&filter)
        .build()
        .expect("request builds");
    assert_eq!(
        req.url().as_str(),
        "https://api.example.com/widgets?color=red&size=5"
    );

    let empty = FindWidgetsFilter {
        color: None,
        size: None,
    };
    let req = reqwest::Client::new()
        .get("https://api.example.com/widgets")
        .query(&empty)
        .build()
        .expect("request builds");
    assert_eq!(
        req.url().as_str(),
        "https://api.example.com/widgets",
        "all-None struct must not leave a dangling `?`"
    );
}

#[test]
fn exploded_and_plain_query_params_coexist() {
    let mut spec = spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "schema": inline_object_schema()
    }));
    spec["paths"]["/widgets"]["get"]["parameters"]
        .as_array_mut()
        .expect("parameters array")
        .push(json!({
            "name": "limit",
            "in": "query",
            "schema": {"type": "integer"}
        }));
    let code = generate_methods(spec);

    assert!(
        code.contains("(\"limit\" , v . to_string ())"),
        "plain scalar param must still go through the pair vector; got:\n{code}"
    );
    assert!(
        code.contains("req = req . query (& v)"),
        "exploded param must still serialize via req.query(&v); got:\n{code}"
    );
}
