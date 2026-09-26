use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use quote::ToTokens;
use serde_json::json;

fn spec() -> serde_json::Value {
    json!({
        "openapi": "3.1.0", "info": {"title": "Bindings", "version": "1"},
        "paths": {
            "/items/{item-id}": {"get": {
                "operationId": "getItem",
                "parameters": [
                    {"name": "item-id", "in": "path", "required": true, "schema": {"type": "string"}},
                    {"name": "mode", "in": "query", "schema": {"type": "string", "enum": ["full", "short"]}}
                ],
                "responses": {
                    "200": {"description": "ok", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Item"}}}},
                    "400": {"description": "bad", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Error"}}}},
                    "404": {"description": "missing", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Missing"}}}}
                }
            }},
            "/download": {"get": {"responses": {"200": {"description": "download", "content": {"application/octet-stream": {"schema": {"type": "string", "format": "binary"}}}}}}}
        },
        "components": {"schemas": {
            "Item": {"type": "object", "properties": {
                "item-id": {"type": "string"}, "item_id": {"type": "integer"},
                "next": {"$ref": "#/components/schemas/Item"},
                "maybe": {"type": ["string", "null"]}
            }},
            "Color": {"type": "string", "enum": ["deep-blue", "deep_blue"]},
            "Identifier": {"type": "integer"},
            "Error": {"type": "object", "properties": {"reason": {"type": "string"}}},
            "Missing": {"type": "object", "properties": {"key": {"type": "string"}}}
        }}
    })
}

fn analysis() -> openapi_to_rust::SchemaAnalysis {
    SchemaAnalyzer::new(spec()).unwrap().analyze().unwrap()
}

fn signature_fingerprint(signature: syn::Signature) -> String {
    let function = syn::ItemFn {
        attrs: Vec::new(),
        vis: syn::Visibility::Inherited,
        sig: signature,
        block: Box::new(syn::Block {
            brace_token: Default::default(),
            stmts: Vec::new(),
        }),
    };
    prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: Vec::new(),
        items: vec![syn::Item::Fn(function)],
    })
}

#[test]
fn metadata_matches_the_emitted_ast_and_is_deterministic() {
    let config = GeneratorConfig {
        enable_sse_client: false,
        ..Default::default()
    };
    let generator = CodeGenerator::new(config);
    let result = generator
        .generate_all_with_bindings(&mut analysis())
        .unwrap();
    let metadata = &result.bindings;
    assert_eq!(metadata.schema_version, 1);
    assert_eq!(metadata.generator_version, openapi_to_rust::VERSION);
    assert_eq!(
        metadata.to_json().unwrap(),
        generator
            .generate_all_with_bindings(&mut analysis())
            .unwrap()
            .bindings
            .to_json()
            .unwrap()
    );
    let ordinary = generator.generate_all(&mut analysis()).unwrap();
    assert_eq!(
        ordinary
            .files
            .iter()
            .map(|f| (&f.path, &f.content))
            .collect::<Vec<_>>(),
        result
            .generation
            .files
            .iter()
            .map(|f| (&f.path, &f.content))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        ordinary.mod_file.content,
        result.generation.mod_file.content
    );
    assert_eq!(ordinary.required_deps, result.generation.required_deps);

    for file in &result.generation.files {
        let module = file.path.file_stem().unwrap().to_str().unwrap();
        let ast: syn::File = syn::parse_str(&file.content).unwrap();
        for item in ast.items {
            match item {
                syn::Item::Struct(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    let symbol = metadata
                        .symbols
                        .iter()
                        .find(|s| s.path == format!("{module}::{}", item.ident))
                        .unwrap();
                    assert_eq!(symbol.kind, "struct");
                    assert_eq!(symbol.fields.len(), item.fields.len());
                    for (actual, expected) in symbol.fields.iter().zip(item.fields.iter()) {
                        assert_eq!(
                            actual.name,
                            expected.ident.as_ref().map(ToString::to_string)
                        );
                        assert_eq!(actual.rust_type, expected.ty.to_token_stream().to_string());
                    }
                }
                syn::Item::Impl(item) => {
                    let syn::Type::Path(owner) = *item.self_ty else {
                        continue;
                    };
                    let name = &owner.path.segments.last().unwrap().ident;
                    for method in item.items {
                        if let syn::ImplItem::Fn(mut method) = method {
                            if matches!(method.vis, syn::Visibility::Public(_)) {
                                let symbol = metadata
                                    .symbols
                                    .iter()
                                    .find(|s| {
                                        s.path == format!("{module}::{name}::{}", method.sig.ident)
                                    })
                                    .unwrap();
                                if method.sig.inputs.trailing_punct() {
                                    method.sig.inputs.pop_punct();
                                }
                                if method.sig.generics.params.trailing_punct() {
                                    method.sig.generics.params.pop_punct();
                                }
                                let metadata_signature: syn::Signature =
                                    syn::parse_str(&symbol.signature.as_ref().unwrap().rust)
                                        .unwrap();
                                assert_eq!(
                                    signature_fingerprint(metadata_signature),
                                    signature_fingerprint(method.sig)
                                );
                            }
                        }
                    }
                }
                syn::Item::Enum(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    let symbol = metadata
                        .symbols
                        .iter()
                        .find(|s| s.path == format!("{module}::{}", item.ident))
                        .unwrap();
                    assert_eq!(symbol.kind, "enum");
                    assert_eq!(
                        symbol
                            .variants
                            .iter()
                            .map(|v| v.name.clone())
                            .collect::<Vec<_>>(),
                        item.variants
                            .iter()
                            .map(|v| v.ident.to_string())
                            .collect::<Vec<_>>()
                    );
                }
                syn::Item::Type(item) if matches!(item.vis, syn::Visibility::Public(_)) => {
                    let symbol = metadata
                        .symbols
                        .iter()
                        .find(|s| s.path == format!("{module}::{}", item.ident))
                        .unwrap();
                    assert_eq!(symbol.kind, "alias");
                    assert_eq!(
                        symbol.rust_type.as_ref().unwrap(),
                        &item.ty.to_token_stream().to_string()
                    );
                }
                _ => {}
            }
        }
    }
    let item = metadata
        .symbols
        .iter()
        .find(|s| s.path == "types::Item")
        .unwrap();
    assert!(
        item.fields
            .iter()
            .any(|f| f.name.as_deref() == Some("item_id")
                && f.wire_name.as_deref() == Some("item-id"))
    );
    assert!(item.fields.iter().any(
        |f| f.name.as_deref() == Some("item_id_2") && f.wire_name.as_deref() == Some("item_id")
    ));
    assert!(item.fields.iter().any(|f| f.rust_type.contains("Box")));
    let method = metadata
        .symbols
        .iter()
        .find(|s| s.path == "client::HttpClient::get_item")
        .unwrap();
    let source = method.operation.as_ref().unwrap();
    assert_eq!(source.source_json_pointer, "/paths/~1items~1{item-id}/get");
    assert_eq!(source.source_operation_id.as_deref(), Some("getItem"));
    assert_eq!(
        source.response_media_type.as_deref(),
        Some("application/json")
    );
    assert!(
        metadata
            .symbols
            .iter()
            .any(|s| s.kind == "enum" && s.path.contains("GetItemMode"))
    );
    assert!(
        metadata
            .symbols
            .iter()
            .any(|s| s.kind == "enum" && s.path.contains("GetItemApiError"))
    );
    let download: Vec<_> = metadata
        .symbols
        .iter()
        .filter_map(|s| s.operation.as_ref())
        .filter(|o| o.source_path == "/download")
        .collect();
    assert!(download.iter().all(|o| o.source_operation_id.is_none()));
    assert!(download.iter().any(|o| o.consumption == "binary_stream"));
    assert!(metadata.reexports.contains(&"client::*".to_string()));
}

#[test]
fn optional_metadata_artifact_never_enters_modules_or_dependencies() {
    let config = GeneratorConfig {
        bindings_metadata: Some("metadata/bindings.json".to_string()),
        enable_sse_client: false,
        ..Default::default()
    };
    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis()).unwrap();
    let artifact = generator.output_artifacts(&result);
    let json = artifact
        .get(std::path::Path::new("metadata/bindings.json"))
        .unwrap();
    let metadata: openapi_to_rust::BindingsMetadata = serde_json::from_str(json).unwrap();
    assert!(!metadata.symbols.is_empty());
    assert!(!result.mod_file.content.contains("metadata"));
    assert!(!result.mod_file.content.contains("bindings"));
    let baseline = CodeGenerator::new(GeneratorConfig {
        enable_sse_client: false,
        ..Default::default()
    })
    .generate_all(&mut analysis())
    .unwrap();
    assert_eq!(result.required_deps, baseline.required_deps);
}

#[test]
fn unsupported_modes_and_unsafe_artifact_names_fail_explicitly() {
    for path in [
        "../bindings.json",
        "/bindings.json",
        "types.rs",
        "mod.rs",
        "REQUIRED_DEPS.toml",
        "types.rs/bindings.json",
        "client.rs/bindings.json",
        "",
    ] {
        let generator = CodeGenerator::new(GeneratorConfig {
            bindings_metadata: Some(path.to_string()),
            ..Default::default()
        });
        assert!(
            generator.generate_all(&mut analysis()).is_err(),
            "accepted {path}"
        );
    }
    let generator = CodeGenerator::new(GeneratorConfig {
        enable_registry: true,
        ..Default::default()
    });
    assert!(
        generator
            .generate_all_with_bindings(&mut analysis())
            .unwrap_err()
            .to_string()
            .contains("unsupported")
    );
    assert!(generator.generate_all(&mut analysis()).is_ok());
    let generator = CodeGenerator::new(GeneratorConfig {
        bindings_metadata: Some("effective.json".to_string()),
        effective_spec: Some("effective.json".into()),
        ..Default::default()
    });
    assert!(generator.generate_all(&mut analysis()).is_err());
}

#[test]
fn enabled_builders_and_extensible_enum_wire_names_follow_emission_plans() {
    let mut document = spec();
    document["components"]["schemas"]["Color"]["enum"] = json!(["deep-blue", "red"]);
    let mut analysis = SchemaAnalyzer::new(document).unwrap().analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        builders: openapi_to_rust::config::BuildersSection {
            enabled: true,
            threshold: 0,
        },
        extensible_enum_overrides: [("Color".to_string(), true)].into_iter().collect(),
        ..Default::default()
    });
    let metadata = generator
        .generate_all_with_bindings(&mut analysis)
        .unwrap()
        .bindings;
    let color = metadata
        .symbols
        .iter()
        .find(|symbol| symbol.path == "types::Color")
        .unwrap();
    assert!(
        color
            .variants
            .iter()
            .any(|variant| variant.name == "DeepBlue"
                && variant.wire_name.as_deref() == Some("deep-blue"))
    );
    assert!(
        color
            .variants
            .iter()
            .any(|variant| variant.name == "Custom" && variant.wire_name.is_none())
    );
    let builder = metadata
        .symbols
        .iter()
        .find(|symbol| {
            symbol.kind == "struct"
                && symbol.path.contains("GetItem")
                && symbol.path.ends_with("Builder")
        })
        .unwrap();
    let origin = builder.operation.as_ref().unwrap();
    let entry = metadata
        .symbols
        .iter()
        .find(|symbol| symbol.path == "client::HttpClient::get_item_builder")
        .unwrap();
    let send = metadata
        .symbols
        .iter()
        .find(|symbol| symbol.path == format!("{}::send", builder.path))
        .unwrap();
    assert_eq!(entry.operation.as_ref().unwrap(), origin);
    assert_eq!(send.operation.as_ref().unwrap(), origin);
    assert!(send.signature.as_ref().unwrap().asynchronous);
    assert!(send.impl_generics.as_deref().unwrap().contains("'a"));
}

#[test]
fn standalone_default_sse_output_compiles_without_a_runtime_file() {
    let document = json!({"openapi": "3.1.0", "info": {"title": "Standalone SSE", "version": "1"},
    "paths": {"/events": {"get": {"operationId": "events", "responses": {"200": {
        "description": "events", "content": {"text/event-stream": {"schema": {"type": "string"}}}
    }}}}}});
    let mut analysis = SchemaAnalyzer::new(document).unwrap().analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig::default());
    let client = generator.generate_http_client(&analysis).unwrap();
    assert!(!client.contains("super::sse"));
    let types = generator.generate(&mut analysis).unwrap();
    let raw_generator = CodeGenerator::new(GeneratorConfig {
        enable_sse_client: false,
        ..Default::default()
    });
    let generated = raw_generator.generate_all(&mut analysis).unwrap();
    let deps = raw_generator
        .output_artifacts(&generated)
        .remove(std::path::Path::new("REQUIRED_DEPS.toml"))
        .unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(temp.path().join("src")).unwrap();
    std::fs::write(temp.path().join("src/types.rs"), types).unwrap();
    std::fs::write(temp.path().join("src/client.rs"), client).unwrap();
    std::fs::write(
        temp.path().join("src/lib.rs"),
        "pub mod types; pub mod client;\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("Cargo.toml"), format!("[package]\nname = \"standalone-sse-bindings-test\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[workspace]\n\n{deps}")).unwrap();
    let output = std::process::Command::new("cargo")
        .args(["check", "--offline", "--quiet"])
        .current_dir(temp.path())
        .env("RUSTC_WRAPPER", "")
        .env("CC_aarch64_apple_darwin", "cc")
        .env("CXX_aarch64_apple_darwin", "c++")
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/standalone-sse-test"),
        )
        .env("CARGO_BUILD_BUILD_DIR", temp.path().join("cargo-build"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn mapping_selection_and_pruning_are_reflected_in_metadata() {
    let mut document = spec();
    document["components"]["schemas"]["Item"]["properties"]["payload"] =
        json!({"type": "string", "format": "byte"});
    let types = openapi_to_rust::TypeMappingConfig {
        byte: openapi_to_rust::ByteStrategy::String,
        ..Default::default()
    };
    let mut analysis =
        SchemaAnalyzer::with_type_mapper(document, openapi_to_rust::TypeMapper::new(types.clone()))
            .unwrap()
            .analyze()
            .unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        types,
        client: Some(openapi_to_rust::config::ClientSection {
            operations: vec!["getItem".to_string()],
            prune_models: true,
        }),
        ..Default::default()
    });
    let result = generator.generate_all_with_bindings(&mut analysis).unwrap();
    assert!(result.generation.pruned_schemas > 0);
    assert!(
        !result
            .bindings
            .symbols
            .iter()
            .any(|symbol| symbol.path == "types::Color")
    );
    assert!(
        !result
            .bindings
            .symbols
            .iter()
            .filter_map(|symbol| symbol.operation.as_ref())
            .any(|operation| operation.source_path == "/download")
    );
    let item = result
        .bindings
        .symbols
        .iter()
        .find(|symbol| symbol.path == "types::Item")
        .unwrap();
    assert_eq!(
        item.fields
            .iter()
            .find(|field| field.name.as_deref() == Some("payload"))
            .unwrap()
            .rust_type,
        "Option < String >"
    );
}

#[test]
fn parsed_sse_runtime_symbols_preserve_target_conditions_and_provenance() {
    let document = json!({"openapi": "3.1.0", "info": {"title": "SSE bindings", "version": "1"},
    "paths": {"/events": {"get": {"operationId": "events", "responses": {"200": {
        "description": "events", "content": {"text/event-stream": {"schema": {"type": "string"}}}
    }}}}}});
    let mut analysis = SchemaAnalyzer::new(document).unwrap().analyze().unwrap();
    let result = CodeGenerator::new(GeneratorConfig::default())
        .generate_all_with_bindings(&mut analysis)
        .unwrap();
    assert!(
        result
            .bindings
            .coverage
            .contains(&"sse_runtime".to_string())
    );
    let streams: Vec<_> = result
        .bindings
        .symbols
        .iter()
        .filter(|symbol| symbol.path == "sse::BoxSseStream")
        .collect();
    assert_eq!(streams.len(), 2);
    assert!(streams.iter().all(|symbol| {
        symbol
            .attributes
            .iter()
            .any(|attribute| attribute.contains("target_arch"))
    }));
    assert!(
        streams
            .iter()
            .any(|symbol| symbol.rust_type.as_ref().unwrap().contains("Send"))
    );
    assert!(
        streams
            .iter()
            .any(|symbol| !symbol.rust_type.as_ref().unwrap().contains("Send"))
    );
    assert!(
        result
            .bindings
            .symbols
            .iter()
            .any(|symbol| symbol.path == "sse::parse_sse_response" && symbol.kind == "function")
    );
    let variants: Vec<_> = result
        .bindings
        .symbols
        .iter()
        .filter_map(|symbol| symbol.operation.as_ref())
        .filter(|operation| operation.source_path == "/events")
        .collect();
    assert!(
        variants
            .iter()
            .any(|operation| operation.consumption == "parsed_sse")
    );
    assert!(
        variants
            .iter()
            .any(|operation| operation.consumption == "raw_event_stream")
    );
    assert!(
        variants
            .iter()
            .all(|operation| operation.source_json_pointer == "/paths/~1events/get")
    );
}
