//! Regression tests for https://github.com/gpu-cli/openapi-to-rust/issues/27
//!
//! Query parameters with object schemas and form-explode semantics previously
//! mapped to `Option<impl AsRef<str>>` and were sent as a single opaque
//! `name=<string>` pair. Per OAS 3.x (style=form + explode=true are the
//! defaults for `in: query`) and RFC 6570 form-explosion, each object property
//! must become its own query pair: `?color=red&size=big` — the parameter name
//! itself never appears in the query string.
//!
//! Now (T14 / openapi-generator-anu): the analyzer assigns each object/array
//! query param a `QuerySerialization` from its style/explode + schema shape,
//! and the client generates the matching wire format — form-exploded objects
//! via `req.query(&v)`, explode=false objects comma-joined, deepObject
//! objects with bracketed keys, form arrays as `Vec<T>` (repeated or
//! comma-joined pairs). Shapes with no defined/wired format (deepObject
//! arrays, arrays of objects, space/pipeDelimited) keep the string fallback.
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
        code.contains("let v = filter") && code.contains("req = req . query (& v)"),
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
fn explode_false_object_serializes_comma_joined() {
    // form + explode=false object: `?filter=color,red,size,big` — one pair,
    // comma-joined key,value list (RFC 6570 form without explosion).
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "explode": false,
        "schema": inline_object_schema()
    })));

    assert!(
        code.contains("filter : Option < FindWidgetsFilter >"),
        "explode=false object param should be typed as the synthesized struct; got:\n{code}"
    );
    assert!(
        code.contains("parts . join (\",\")"),
        "explode=false object must comma-join key,value parts; got:\n{code}"
    );
    assert!(
        code.contains("query_params . push ((\"filter\" . to_string () , parts . join (\",\")))"),
        "explode=false object keeps the parameter name as the single key; got:\n{code}"
    );
}

#[test]
fn deep_object_style_serializes_bracketed_keys() {
    // deepObject: `?filter[color]=red&filter[size]=5`.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "filter",
        "in": "query",
        "style": "deepObject",
        "explode": true,
        "schema": inline_object_schema()
    })));

    assert!(
        code.contains("filter : Option < FindWidgetsFilter >"),
        "deepObject param should be typed as the synthesized struct; got:\n{code}"
    );
    assert!(
        code.contains("format ! (\"{}[{}]\" , \"filter\" , k)"),
        "deepObject must emit bracketed `filter[key]` query keys; got:\n{code}"
    );
    assert!(
        code.contains("req = req . query (& deep_params)"),
        "deepObject pairs must be appended to the request; got:\n{code}"
    );
}

#[test]
fn deep_object_array_keeps_string_fallback() {
    // deepObject on an *array* schema (stripe's `expand[]`) is undefined in
    // OAS 3.x — keeps the opaque string fallback.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "expand",
        "in": "query",
        "style": "deepObject",
        "explode": true,
        "schema": {"type": "array", "items": {"type": "string"}}
    })));

    assert!(
        code.contains("expand : Option < impl AsRef < str > >"),
        "deepObject array param should keep the string fallback; got:\n{code}"
    );
}

#[test]
fn form_exploded_array_repeats_pairs() {
    // form + explode=true array (the OAS defaults): `?tags=a&tags=b`.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "tags",
        "in": "query",
        "schema": {"type": "array", "items": {"type": "string"}}
    })));

    assert!(
        code.contains("tags : Option < Vec < String > >"),
        "exploded string array should be typed Vec<String>; got:\n{code}"
    );
    assert!(
        code.contains("for item in v")
            && code.contains("\"tags\" . to_string () , item . to_string ()"),
        "exploded array must push one pair per element; got:\n{code}"
    );
}

#[test]
fn form_exploded_array_types_integer_items_through_type_mapper() {
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "ids",
        "in": "query",
        "required": true,
        "schema": {"type": "array", "items": {"type": "integer", "format": "int32"}}
    })));

    assert!(
        code.contains("ids : Vec < i32 >"),
        "required int32 array should be a bare Vec<i32>; got:\n{code}"
    );
    assert!(
        code.contains("for item in v"),
        "required exploded array iterates the bound value; got:\n{code}"
    );
}

#[test]
fn form_noexplode_array_joins_with_commas() {
    // form + explode=false array: `?tags=a,b,c`; empty vectors use a marker
    // so a generated server can distinguish Some(empty) from None.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "tags",
        "in": "query",
        "explode": false,
        "schema": {"type": "array", "items": {"type": "string"}}
    })));

    assert!(
        code.contains("tags : Option < Vec < String > >"),
        "non-exploded string array should be typed Vec<String>; got:\n{code}"
    );
    assert!(
        code.contains(". join (\",\")"),
        "non-exploded array must comma-join its items; got:\n{code}"
    );
    assert!(
        code.contains("if v . is_empty ()") && code.contains("format ! (\"{}[]\" , \"tags\")"),
        "empty non-exploded arrays must emit the zero-cardinality marker; got:\n{code}"
    );
}

#[test]
fn array_of_ref_string_enum_items_uses_enum_type() {
    let mut spec = spec_with_filter_param(json!({
        "name": "status",
        "in": "query",
        "schema": {"type": "array", "items": {"$ref": "#/components/schemas/WidgetStatus"}}
    }));
    spec["components"] = json!({
        "schemas": {
            "WidgetStatus": {"type": "string", "enum": ["active", "retired"]}
        }
    });
    let code = generate_methods(spec);

    assert!(
        code.contains("status : Option < Vec < WidgetStatus > >"),
        "array of $ref string-enum items should be Vec<Enum>; got:\n{code}"
    );
}

#[test]
fn inline_array_of_scalar_alias_items_preserves_alias_and_pruning_root() {
    let mut spec = spec_with_filter_param(json!({
        "name": "tags",
        "in": "query",
        "schema": {
            "type": "array",
            "items": {"$ref": "#/components/schemas/Identifier"}
        }
    }));
    spec["components"] = json!({
        "schemas": {
            "Identifier": {"type": "string", "format": "xid"},
            "Unused": {"type": "string"}
        }
    });

    let code = generate_methods(spec.clone());
    assert!(
        code.contains("tags : Option < Vec < Identifier > >"),
        "direct scalar alias items should preserve their named type; got:\n{code}"
    );
    assert!(
        code.contains("for item in v")
            && code.contains("\"tags\" . to_string () , item . to_string ()"),
        "exploded scalar aliases must still emit one pair per item; got:\n{code}"
    );

    let mut analyzer = SchemaAnalyzer::new(spec).expect("analyzer construction");
    let analysis = analyzer.analyze().expect("analysis");
    let operation = analysis.operations.get("findWidgets").expect("operation");
    let reachable = openapi_to_rust::server::codegen::reachable_schemas(&analysis, &[operation]);
    assert!(reachable.contains("Identifier"));
    assert!(!reachable.contains("Unused"));
}

#[test]
fn reusable_array_of_transitive_scalar_alias_items_preserves_outer_alias() {
    let mut spec = spec_with_filter_param(json!({
        "name": "scores",
        "in": "query",
        "explode": false,
        "schema": {"$ref": "#/components/schemas/ScoreListAlias"}
    }));
    spec["components"] = json!({
        "schemas": {
            "ScoreListAlias": {"$ref": "#/components/schemas/ScoreList"},
            "ScoreList": {
                "type": "array",
                "items": {"$ref": "#/components/schemas/PublicScore"}
            },
            "PublicScore": {"$ref": "#/components/schemas/Score"},
            "Score": {"type": "integer", "format": "int32"}
        }
    });

    let code = generate_methods(spec);
    assert!(
        code.contains("scores : Option < Vec < PublicScore > >"),
        "transitive scalar aliases should retain the array item's outer alias; got:\n{code}"
    );
    assert!(
        code.contains("parts . join (\",\")"),
        "explode=false alias arrays must retain comma-joined serialization; got:\n{code}"
    );
    assert!(
        !code.contains("scores : Option < impl AsRef < str > >"),
        "supported alias arrays must not use the opaque string fallback; got:\n{code}"
    );
}

#[test]
fn array_of_objects_keeps_string_fallback() {
    // Arrays of objects have no defined form serialization — fallback.
    let code = generate_methods(spec_with_filter_param(json!({
        "name": "filters",
        "in": "query",
        "schema": {"type": "array", "items": {"type": "object", "properties": {"k": {"type": "string"}}}}
    })));

    assert!(
        code.contains("filters : Option < impl AsRef < str > >"),
        "array-of-objects param should keep the string fallback; got:\n{code}"
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

/// Runtime pin for the deepObject wire format: reqwest percent-encodes the
/// brackets in `filter[color]` (form-urlencoded rules), which servers decode
/// transparently — Stripe et al. accept `filter%5Bcolor%5D=red`.
#[test]
fn reqwest_query_percent_encodes_deep_object_brackets() {
    let deep: Vec<(String, String)> = vec![("filter[color]".to_string(), "red".to_string())];
    let req = reqwest::Client::new()
        .get("https://api.example.com/widgets")
        .query(&deep)
        .build()
        .expect("request builds");
    assert_eq!(
        req.url().as_str(),
        "https://api.example.com/widgets?filter%5Bcolor%5D=red"
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
        code.contains("(\"limit\" . to_string () , v . to_string ())"),
        "plain scalar param must still go through the pair vector; got:\n{code}"
    );
    assert!(
        code.contains("req = req . query (& v)"),
        "exploded param must still serialize via req.query(&v); got:\n{code}"
    );
}
