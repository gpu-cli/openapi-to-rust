//! An error variant that wraps a source via `#[from]`/`#[source]` must not also
//! format that source into its own message, or chain-aware reporters print the
//! cause twice (gpu-cli/openapi-to-rust#112).

use openapi_to_rust::GeneratorError;
use openapi_to_rust::error::error_chain;
use openapi_to_rust::streaming::{HttpMethod, StreamingConfig, StreamingEndpoint};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

/// Every `#[error("...")]` message in `source` that formats its variant's own
/// error source, as `Enum::Variant` names.
fn variants_displaying_their_source(source: &str) -> Vec<String> {
    let file = syn::parse_file(source).expect("source should parse");
    let mut offenders = Vec::new();
    for item in file.items {
        let syn::Item::Enum(item) = item else {
            continue;
        };
        for variant in &item.variants {
            let Some(message) = variant.attrs.iter().find_map(|attr| {
                if !attr.path().is_ident("error") {
                    return None;
                }
                attr.parse_args::<syn::LitStr>().ok().map(|lit| lit.value())
            }) else {
                continue;
            };
            let source_placeholder =
                variant
                    .fields
                    .iter()
                    .enumerate()
                    .find_map(|(index, field)| {
                        let is_source =
                            field.attrs.iter().any(|attr| {
                                attr.path().is_ident("from") || attr.path().is_ident("source")
                            }) || field.ident.as_ref().is_some_and(|ident| ident == "source");
                        is_source.then(|| match &field.ident {
                            Some(ident) => format!("{{{ident}"),
                            None => format!("{{{index}"),
                        })
                    });
            if source_placeholder.is_some_and(|placeholder| message.contains(&placeholder)) {
                offenders.push(format!("{}::{}", item.ident, variant.ident));
            }
        }
    }
    offenders
}

#[test]
fn generated_error_enums_leave_sources_out_of_their_messages() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "error sources", "version": "1" },
        "paths": {
            "/events": { "get": {
                "operationId": "streamEvents",
                "responses": { "200": {
                    "description": "events",
                    "content": { "text/event-stream": { "schema": {
                        "$ref": "#/components/schemas/Event"
                    } } }
                } }
            } }
        },
        "components": { "schemas": {
            "Event": {
                "type": "object",
                "properties": { "message": { "type": "string" } }
            }
        } }
    });
    let mut analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: true,
        streaming_config: Some(StreamingConfig {
            endpoints: vec![StreamingEndpoint {
                operation_id: "streamEvents".into(),
                path: "/events".into(),
                http_method: HttpMethod::Get,
                event_union_type: "Event".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();

    let mut checked = Vec::new();
    for file in &result.files {
        let offenders = variants_displaying_their_source(&file.content);
        assert!(
            offenders.is_empty(),
            "{} formats wrapped sources into messages: {offenders:?}",
            file.path.display()
        );
        checked.push(file.path.display().to_string());
    }
    for expected in ["client.rs", "streaming.rs"] {
        assert!(
            checked.iter().any(|path| path == expected),
            "{expected} should be generated, got {checked:?}"
        );
    }
}

#[test]
fn crate_error_enums_leave_sources_out_of_their_messages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for path in ["src/error.rs", "src/http_error.rs", "src/server/edit.rs"] {
        let source = std::fs::read_to_string(root.join(path)).unwrap();
        let offenders = variants_displaying_their_source(&source);
        assert!(
            offenders.is_empty(),
            "{path} formats wrapped sources into messages: {offenders:?}"
        );
    }
}

#[test]
fn error_chain_prints_each_cause_once() {
    let error = GeneratorError::from(std::io::Error::other("disk full"));
    assert_eq!(error_chain(&error), "IO error: disk full");

    let error = GeneratorError::from(serde_json::from_str::<u8>("x").unwrap_err());
    let rendered = error_chain(&error);
    assert!(
        rendered.starts_with("Failed to parse OpenAPI spec: expected value"),
        "unexpected rendering: {rendered}"
    );
    assert_eq!(rendered.matches("expected value").count(), 1, "{rendered}");
}
