//! Enum values whose names collide once converted to an identifier, such as
//! `amber-strict` and `amber+strict`, get numbered variants. The number must
//! not be joined with an underscore after a letter: `AmberStrict_2` trips
//! `rustc`'s `non_camel_case_types` lint, while `AmberStrict2` doesn't.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn spec() -> serde_json::Value {
    json!({
        "openapi": "3.0.3",
        "info": { "title": "variant collisions", "version": "1" },
        "paths": {
            "/zones": { "get": {
                "operationId": "listZones",
                "parameters": [{
                    "name": "order",
                    "in": "query",
                    "schema": { "type": "string", "enum": ["created_at", "-created_at"] }
                }],
                "responses": { "204": { "description": "ok" } }
            }}
        },
        "components": { "schemas": {
            "Mode": { "type": "string", "enum": ["amber-strict", "amber+strict", "amber_strict"] },
            "Version": { "type": "string", "enum": ["v1", "V1"] }
        }}
    })
}

/// The generated types and client, parsed.
fn generate() -> (syn::File, syn::File) {
    let mut analyzer = SchemaAnalyzer::new(spec()).expect("analyzer");
    let mut analysis = analyzer.analyze().expect("analysis");
    let generator = CodeGenerator::new(GeneratorConfig::default());
    let types = generator.generate(&mut analysis).expect("types");
    let client = generator
        .generate_http_client(&analysis)
        .expect("client generation");
    (
        syn::parse_file(&types).expect("types parse"),
        syn::parse_file(&client).expect("client parses"),
    )
}

/// The variants of the generated enum `name`.
fn variants(file: &syn::File, name: &str) -> Vec<String> {
    fn find<'a>(items: &'a [syn::Item], name: &str) -> Option<&'a syn::ItemEnum> {
        items.iter().find_map(|item| match item {
            syn::Item::Enum(item) if item.ident == name => Some(item),
            syn::Item::Mod(module) => module
                .content
                .as_ref()
                .and_then(|(_, items)| find(items, name)),
            _ => None,
        })
    }
    find(&file.items, name)
        .unwrap_or_else(|| panic!("no enum {name}"))
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect()
}

/// `rustc`'s `non_camel_case_types` check.
fn is_camel_case(name: &str) -> bool {
    let name = name.trim_matches('_');
    let chars: Vec<char> = name.chars().collect();
    !chars.first().is_some_and(|c| c.is_lowercase())
        && !name.contains("__")
        && !chars.windows(2).any(|pair| {
            let has_case = |c: char| c.is_lowercase() || c.is_uppercase();
            (has_case(pair[0]) && pair[1] == '_') || (pair[0] == '_' && has_case(pair[1]))
        })
}

#[test]
fn colliding_enum_values_get_camel_case_numbered_variants() {
    let (types, client) = generate();

    assert_eq!(
        variants(&types, "Mode"),
        ["AmberStrict", "AmberStrict2", "AmberStrict3"]
    );
    // After a digit, the underscore keeps the numbers apart: `V1_2`, not `V12`.
    assert_eq!(variants(&types, "Version"), ["V1", "V1_2"]);
    assert_eq!(
        variants(&client, "ListZonesOrder"),
        ["CreatedAt", "CreatedAt2"]
    );

    for (file, name) in [
        (&types, "Mode"),
        (&types, "Version"),
        (&client, "ListZonesOrder"),
    ] {
        for variant in variants(file, name) {
            assert!(
                is_camel_case(&variant),
                "{name}::{variant} isn't camel case"
            );
        }
    }
}
