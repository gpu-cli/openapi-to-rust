use openapi_to_rust::analysis::{SchemaAnalysis, SchemaRef, SchemaType};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn analyze(spec: Value) -> SchemaAnalysis {
    SchemaAnalyzer::new(spec)
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze")
}

fn union_targets(analysis: &SchemaAnalysis, name: &str) -> Vec<String> {
    match &analysis.schemas[name].schema_type {
        SchemaType::Union { variants, .. } => variants
            .iter()
            .map(|SchemaRef { target, .. }| target.clone())
            .collect(),
        other => panic!("{name} should be an untagged union, got {other:?}"),
    }
}

#[test]
fn discriminated_equal_inline_branches_keep_distinct_provenance() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "discriminated-branch-collision", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "RepeatedDiscriminated": {
                    "oneOf": [
                        false,
                        {
                            "type": "object",
                            "properties": { "payload": { "type": "string" } },
                            "required": ["payload"]
                        },
                        {
                            "type": "object",
                            "properties": { "payload": { "type": "string" } },
                            "required": ["payload"]
                        }
                    ],
                    "discriminator": {
                        "propertyName": "kind",
                        "mapping": {
                            "first": "#/components/schemas/RepeatedDiscriminated/variant_1",
                            "second": "#/components/schemas/RepeatedDiscriminated/variant_2"
                        }
                    }
                }
            }
        }
    });

    let mut analysis = analyze(spec);
    let SchemaType::DiscriminatedUnion { variants, .. } =
        &analysis.schemas["RepeatedDiscriminated"].schema_type
    else {
        panic!("RepeatedDiscriminated should be a discriminated union");
    };

    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].discriminator_value, "first");
    assert_eq!(variants[1].discriminator_value, "second");
    assert_eq!(variants[0].schema_ref, "inline_1");
    assert_eq!(variants[1].schema_ref, "inline_2");
    assert_ne!(variants[0].type_name, variants[1].type_name);
    assert!(analysis.schemas.contains_key(&variants[0].type_name));
    assert!(analysis.schemas.contains_key(&variants[1].type_name));

    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("distinct discriminated branch payloads should generate");
}

#[test]
fn inferred_anyof_discriminator_keeps_branch_scoped_payloads() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "inferred-anyof-branch-collision", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "InferredAnyOf": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "kind": { "const": "alpha" },
                                "payload": { "type": "string" }
                            },
                            "required": ["kind", "payload"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "kind": { "const": "beta" },
                                "payload": { "type": "string" }
                            },
                            "required": ["kind", "payload"]
                        }
                    ]
                }
            }
        }
    });

    let mut analysis = analyze(spec);
    let SchemaType::DiscriminatedUnion {
        discriminator_field,
        variants,
    } = &analysis.schemas["InferredAnyOf"].schema_type
    else {
        panic!("InferredAnyOf should infer a discriminated union");
    };

    assert_eq!(discriminator_field, "kind");
    assert_eq!(variants.len(), 2);
    assert_eq!(variants[0].discriminator_value, "alpha");
    assert_eq!(variants[1].discriminator_value, "beta");
    assert_ne!(variants[0].type_name, variants[1].type_name);

    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("inferred anyOf branch payloads should generate");
}

#[test]
fn untagged_oneof_and_anyof_equal_inline_branches_do_not_alias() {
    let repeated_branch = json!({
        "type": "object",
        "properties": { "common": { "type": "string" } },
        "required": ["common"]
    });
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "untagged-branch-collision", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "RepeatedOneOf": {
                    "oneOf": [repeated_branch.clone(), repeated_branch.clone()]
                },
                "RepeatedAnyOf": {
                    "anyOf": [repeated_branch.clone(), repeated_branch]
                }
            }
        }
    });

    let mut analysis = analyze(spec);
    let one_of_targets = union_targets(&analysis, "RepeatedOneOf");
    let any_of_targets = union_targets(&analysis, "RepeatedAnyOf");

    assert_eq!(one_of_targets.len(), 2);
    assert_ne!(one_of_targets[0], one_of_targets[1]);
    assert_eq!(any_of_targets.len(), 2);
    assert_ne!(any_of_targets[0], any_of_targets[1]);
    for target in one_of_targets.iter().chain(&any_of_targets) {
        assert!(analysis.schemas.contains_key(target));
    }

    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("distinct untagged branch payloads should generate");
}

#[test]
fn source_field_union_does_not_emit_its_int32_branch_as_a_self_variant() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "cloudflare-source-field", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "ListField": {
                    "type": "object",
                    "properties": { "items": { "$ref": "#/components/schemas/SourceField" } },
                    "required": ["items"]
                },
                "SourceField": {
                    "type": "object",
                    "properties": { "name": { "type": "string" } },
                    "required": ["name"],
                    "discriminator": { "propertyName": "type" },
                    "anyOf": [
                        {
                            "title": "Int32",
                            "type": "object",
                            "properties": { "type": { "type": "string", "enum": ["int32"] } },
                            "required": ["type"]
                        },
                        {
                            "title": "Int64",
                            "type": "object",
                            "properties": { "type": { "type": "string", "enum": ["int64"] } },
                            "required": ["type"]
                        },
                        {
                            "title": "List",
                            "allOf": [
                                { "$ref": "#/components/schemas/ListField" },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "type": "string", "enum": ["list"] }
                                    },
                                    "required": ["type"]
                                }
                            ]
                        }
                    ]
                }
            }
        }
    });

    let mut analysis = analyze(spec);
    let SchemaType::Object {
        variant: Some(variant),
        ..
    } = &analysis.schemas["SourceField"].schema_type
    else {
        panic!("SourceField should retain its base fields and flattened variants");
    };
    let union_name = variant.target.clone();
    let SchemaType::DiscriminatedUnion { variants, .. } =
        &analysis.schemas[&union_name].schema_type
    else {
        panic!("SourceField flattened variant should be discriminated");
    };

    assert_eq!(variants.len(), 3);
    assert!(variants.iter().all(|branch| branch.type_name != union_name));
    assert_eq!(
        variants
            .iter()
            .map(|branch| branch.type_name.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );

    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("recursive SourceField/ListField models should generate");
    assert!(!generated.contains(&format!("Box<{union_name}>")));
}
