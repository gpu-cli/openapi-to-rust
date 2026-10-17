#![recursion_limit = "256"]

use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn object_union_allof_spec() -> Value {
    let mut spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "object union allOf", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "RuleFeature": {
                    "allOf": [
                        {
                            "type": "object",
                            "required": ["name"],
                            "properties": { "name": { "type": "string" } }
                        },
                        {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "required": ["type"],
                                    "properties": {
                                        "type": { "type": "string", "const": "CARD" }
                                    }
                                },
                                {
                                    "type": "object",
                                    "required": ["type", "scope"],
                                    "properties": {
                                        "type": { "type": "string", "const": "VELOCITY" },
                                        "scope": { "type": "string" }
                                    }
                                }
                            ],
                            "discriminator": { "propertyName": "type" }
                        }
                    ]
                },
                "StringFilter": {
                    "type": "object",
                    "required": ["type", "op", "value"],
                    "properties": {
                        "type": { "type": "string", "enum": ["string"] },
                        "op": { "type": "string" },
                        "value": { "type": "string" }
                    }
                },
                "NumberFilter": {
                    "type": "object",
                    "required": ["type", "op", "value"],
                    "properties": {
                        "type": { "type": "string", "enum": ["number"] },
                        "op": { "type": "string" },
                        "value": { "type": "number" }
                    }
                },
                "ValueFilter": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/StringFilter" },
                        { "$ref": "#/components/schemas/NumberFilter" }
                    ]
                },
                "CustomFieldFilter": {
                    "allOf": [
                        { "$ref": "#/components/schemas/ValueFilter" },
                        {
                            "type": "object",
                            "required": ["key"],
                            "properties": { "key": { "type": "string" } }
                        }
                    ]
                },
                "SignerBase": {
                    "type": "object",
                    "properties": {
                        "email": { "type": "string", "nullable": true },
                        "language": { "type": "string" }
                    }
                },
                "Signer": {
                    "type": "object",
                    "required": ["email"],
                    "allOf": [
                        { "$ref": "#/components/schemas/SignerBase" },
                        {
                            "type": "object",
                            "properties": { "viewed": { "type": "boolean" } }
                        }
                    ]
                },
                "RootSibling": {
                    "type": "object",
                    "required": ["root_value"],
                    "properties": { "root_value": { "type": "string" } },
                    "allOf": [
                        {
                            "type": "object",
                            "required": ["child_value"],
                            "properties": { "child_value": { "type": "integer" } }
                        }
                    ]
                },
                "RecordSharedFields": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "ttl": { "type": "number" }
                    }
                },
                "ARecord": {
                    "allOf": [
                        { "$ref": "#/components/schemas/RecordSharedFields" },
                        {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "type": { "type": "string", "enum": ["A"] }
                            }
                        }
                    ]
                },
                "MXRecord": {
                    "allOf": [
                        { "$ref": "#/components/schemas/RecordSharedFields" },
                        {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "priority": { "type": "number" },
                                "type": { "type": "string", "enum": ["MX"] }
                            }
                        }
                    ]
                },
                "CAARecord": {
                    "allOf": [
                        { "$ref": "#/components/schemas/RecordSharedFields" },
                        {
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "data": {
                                    "type": "object",
                                    "properties": {
                                        "flags": { "type": "number" },
                                        "tag": { "type": "string" }
                                    }
                                },
                                "type": { "type": "string", "enum": ["CAA"] }
                            }
                        }
                    ]
                },
                "RecordWithoutData": {
                    "oneOf": [
                        { "$ref": "#/components/schemas/ARecord" },
                        { "$ref": "#/components/schemas/MXRecord" }
                    ]
                },
                "RecordWithData": {
                    "oneOf": [{ "$ref": "#/components/schemas/CAARecord" }]
                },
                "RecordResponse": {
                    "type": "object",
                    "required": ["name", "type", "content", "data", "ttl"],
                    "allOf": [
                        {
                            "anyOf": [
                                { "$ref": "#/components/schemas/RecordWithoutData" },
                                { "$ref": "#/components/schemas/RecordWithData" }
                            ]
                        },
                        {
                            "type": "object",
                            "required": ["id"],
                            "properties": { "id": { "type": "string" } }
                        }
                    ]
                },
                "SiblingRecordResponse": {
                    "type": "object",
                    "required": ["name", "type", "content", "data", "ttl", "id"],
                    "properties": { "id": { "type": "string" } },
                    "anyOf": [
                        { "$ref": "#/components/schemas/RecordWithoutData" },
                        { "$ref": "#/components/schemas/RecordWithData" }
                    ]
                },
                "FilterLeaf": {
                    "type": "object",
                    "required": ["key", "operation"],
                    "properties": {
                        "key": { "type": "string" },
                        "operation": { "type": "string" },
                        "value": { "type": "string" }
                    }
                },
                "FilterNode": {
                    "description": "A leaf filter or a nested group.",
                    "type": "object",
                    "anyOf": [
                        {
                            "type": "object",
                            "required": ["kind", "filters"],
                            "properties": {
                                "kind": { "type": "string", "enum": ["group"] },
                                "filters": {
                                    "type": "array",
                                    "items": { "$ref": "#/components/schemas/FilterNode" }
                                }
                            }
                        },
                        { "$ref": "#/components/schemas/FilterLeaf" }
                    ]
                },
                "RestatedFilter": {
                    "allOf": [
                        { "$ref": "#/components/schemas/FilterNode" },
                        {
                            "description": "Supports nested groups.",
                            "anyOf": [
                                {
                                    "description": "A group of child filters.",
                                    "type": "object",
                                    "required": ["kind", "filters"],
                                    "properties": {
                                        "kind": {
                                            "description": "Marks a group.",
                                            "type": "string",
                                            "enum": ["group"]
                                        },
                                        "filters": {
                                            "description": "Child filters.",
                                            "type": "array",
                                            "items": { "$ref": "#/components/schemas/FilterNode" }
                                        }
                                    }
                                },
                                { "$ref": "#/components/schemas/FilterLeaf" }
                            ]
                        }
                    ]
                },
                "GitpodOutputSpec": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "key": { "type": "string" },
                        "command": { "type": "string" },
                        "prompt": { "type": "string" },
                        "boolean": { "type": "object", "properties": {} },
                        "string": { "type": "object", "properties": {} }
                    },
                    "allOf": [
                        {
                            "anyOf": [
                                { "required": ["command"] },
                                { "required": ["prompt"] },
                                {
                                    "not": {
                                        "anyOf": [
                                            { "required": ["command"] },
                                            { "required": ["prompt"] }
                                        ]
                                    }
                                }
                            ]
                        },
                        {
                            "anyOf": [
                                { "required": ["boolean"] },
                                { "required": ["string"] },
                                {
                                    "not": {
                                        "anyOf": [
                                            { "required": ["boolean"] },
                                            { "required": ["string"] }
                                        ]
                                    }
                                }
                            ]
                        }
                    ]
                }
            }
        }
    });
    let schemas = spec["components"]["schemas"]
        .as_object_mut()
        .expect("schema map");
    let reference = json!({ "$ref": "#/components/schemas/FilterNode" });
    let inline = schemas["FilterNode"].clone();
    for (name, members) in [
        ("RepeatedFilterWithOwner", json!([reference, reference])),
        ("InlineRestatedFilterWithOwner", json!([inline, reference])),
    ] {
        schemas.insert(
            name.into(),
            json!({
                "type": "object",
                "properties": { "owner_field": { "type": "string" } },
                "required": ["owner_field"],
                "allOf": members
            }),
        );
    }
    schemas.insert(
        "MixedFilter".into(),
        json!({"anyOf": [
            {"type": "string"},
            {"type": "object", "properties": {"key": {"type": "string"}}, "required": ["key"]}
        ]}),
    );
    for (name, mut owner) in [
        ("RepeatedMixedFilterObject", json!({"type": "object"})),
        (
            "RepeatedMixedFilterNotString",
            json!({"not": {"type": "string"}}),
        ),
    ] {
        owner["allOf"] = json!([
            {"$ref": "#/components/schemas/MixedFilter"},
            {"$ref": "#/components/schemas/MixedFilter"}
        ]);
        schemas.insert(name.into(), owner);
    }
    spec
}

fn analyze() -> openapi_to_rust::SchemaAnalysis {
    SchemaAnalyzer::new(object_union_allof_spec())
        .expect("object union allOf spec should parse")
        .analyze()
        .expect("object union allOf spec should analyze")
}

#[test]
fn allof_objects_retain_inline_referenced_and_root_sibling_shapes() {
    let analysis = analyze();
    for name in ["RuleFeature", "CustomFieldFilter"] {
        let SchemaType::Object {
            properties,
            variant,
            ..
        } = &analysis.schemas[name].schema_type
        else {
            panic!("{name} should be an object with a flattened union");
        };
        assert!(variant.is_some(), "{name} must retain its union member");
        assert!(
            properties.contains_key(if name == "RuleFeature" { "name" } else { "key" }),
            "{name} must retain its sibling property"
        );
    }

    let SchemaType::Object {
        properties,
        required,
        ..
    } = &analysis.schemas["Signer"].schema_type
    else {
        panic!("Signer should be a merged object");
    };
    assert!(properties["email"].nullable);
    assert!(required.contains("email"));

    let SchemaType::Object {
        properties,
        required,
        ..
    } = &analysis.schemas["RootSibling"].schema_type
    else {
        panic!("RootSibling should be a merged object");
    };
    assert!(properties.contains_key("root_value"));
    assert!(properties.contains_key("child_value"));
    assert!(required.contains("root_value"));
    assert!(required.contains("child_value"));

    for name in ["RecordResponse", "SiblingRecordResponse"] {
        let SchemaType::Object {
            properties: record,
            variant,
            ..
        } = &analysis.schemas[name].schema_type
        else {
            panic!("{name} should be an object with a flattened union");
        };
        assert!(variant.is_some(), "{name} must retain its union member");
        assert!(record.contains_key("id"));
        for field in ["name", "type", "content", "data", "ttl"] {
            assert!(
                !record.contains_key(field),
                "{name}.{field} is declared by the union branches, not the shared fields"
            );
        }
    }

    let SchemaType::Object {
        properties: output_spec,
        ..
    } = &analysis.schemas["GitpodOutputSpec"].schema_type
    else {
        panic!("GitpodOutputSpec should retain its root object shape");
    };
    for field in ["key", "command", "prompt", "boolean", "string"] {
        assert!(
            output_spec.contains_key(field),
            "Gitpod root sibling field {field} was dropped"
        );
    }
}

#[test]
fn generated_allof_objects_round_trip_union_fields_and_required_nulls_exactly() {
    let mut analysis = analyze();
    let code = CodeGenerator::new(GeneratorConfig {
        module_name: "object_union_allof".into(),
        enable_async_client: false,
        ..Default::default()
    })
    .generate(&mut analysis)
    .expect("object union allOf types should generate");
    assert!(
        code.contains("struct __CustomFieldFilterBase")
            && code.contains("let mut variant_input = value"),
        "allOf unions need complete-object base/variant decoding. Code:\n{code}"
    );
    let signer_start = code.find("pub struct Signer {").expect("Signer struct");
    let signer_end = code[signer_start..]
        .find("\n}")
        .map(|offset| signer_start + offset)
        .expect("Signer end");
    let signer = &code[signer_start..signer_end];
    assert!(
        signer.starts_with("pub struct Signer {\n    pub email: Option<String>,"),
        "required nullable email must serialize explicit null: {signer}"
    );

    let temp = tempfile::TempDir::new().expect("scratch crate");
    std::fs::create_dir_all(temp.path().join("src")).expect("scratch src");
    std::fs::write(temp.path().join("src/generated.rs"), code).expect("generated module");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "object-union-allof-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("scratch manifest");
    std::fs::write(
        temp.path().join("src/main.rs"),
        r##"#![allow(dead_code)]
mod generated;

fn round_trip<T>(input: serde_json::Value)
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    let hydrated: T = serde_json::from_value(input.clone()).unwrap();
    let output = serde_json::to_value(hydrated).unwrap();
    assert_eq!(output, input);
    let hydrated_again: T = serde_json::from_value(output.clone()).unwrap();
    assert_eq!(serde_json::to_value(hydrated_again).unwrap(), output);
}

fn round_trip_records<T>()
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    round_trip::<T>(serde_json::json!({
        "id": "r1",
        "name": "www.example.com",
        "type": "A",
        "content": "198.51.100.4",
        "ttl": 1.0
    }));
    round_trip::<T>(serde_json::json!({
        "id": "r2",
        "name": "example.com",
        "type": "MX",
        "content": "mail.example.com",
        "priority": 10.0,
        "ttl": 3600.0
    }));
    round_trip::<T>(serde_json::json!({
        "id": "r3",
        "name": "example.com",
        "type": "CAA",
        "content": "0 issue \"letsencrypt.org\"",
        "data": { "flags": 0.0, "tag": "issue" },
        "ttl": 1.0
    }));
}

fn main() {
    round_trip::<generated::RuleFeature>(serde_json::json!({
        "name": "card",
        "type": "CARD"
    }));
    round_trip::<generated::RuleFeature>(serde_json::json!({
        "name": "velocity",
        "type": "VELOCITY",
        "scope": "card"
    }));
    round_trip::<generated::CustomFieldFilter>(serde_json::json!({
        "key": "status",
        "type": "string",
        "op": "eq",
        "value": "ready"
    }));
    round_trip::<generated::CustomFieldFilter>(serde_json::json!({
        "key": "amount",
        "type": "number",
        "op": "gt",
        "value": 10.5
    }));
    round_trip::<generated::Signer>(serde_json::json!({
        "email": null,
        "language": "en",
        "viewed": true
    }));
    round_trip::<generated::RootSibling>(serde_json::json!({
        "root_value": "root",
        "child_value": 7
    }));
    round_trip_records::<generated::RecordResponse>();
    round_trip_records::<generated::SiblingRecordResponse>();
    round_trip::<generated::GitpodOutputSpec>(serde_json::json!({
        "key": "coverage",
        "command": "collect-coverage",
        "boolean": {}
    }));
    round_trip::<generated::GitpodOutputSpec>(serde_json::json!({}));
    round_trip::<generated::RestatedFilter>(serde_json::json!({
        "kind": "group",
        "filters": [
            { "key": "service", "operation": "eq", "value": "api" },
            { "kind": "group", "filters": [] }
        ]
    }));
    round_trip::<generated::RestatedFilter>(serde_json::json!({
        "key": "status",
        "operation": "exists"
    }));
    round_trip::<generated::RepeatedFilterWithOwner>(serde_json::json!({
        "owner_field": "request-1",
        "key": "status",
        "operation": "exists"
    }));
    round_trip::<generated::InlineRestatedFilterWithOwner>(serde_json::json!({
        "owner_field": "request-2",
        "kind": "group",
        "filters": [{ "key": "status", "operation": "exists" }]
    }));
    assert!(serde_json::from_value::<generated::RepeatedFilterWithOwner>(
        serde_json::json!({ "key": "status", "operation": "exists" })
    ).is_err());
    assert!(serde_json::from_value::<generated::InlineRestatedFilterWithOwner>(
        serde_json::json!({ "kind": "group", "filters": [] })
    ).is_err());
    round_trip::<generated::RepeatedMixedFilterObject>(serde_json::json!({ "key": "status" }));
    assert!(serde_json::from_value::<generated::RepeatedMixedFilterObject>(
        serde_json::json!("scalar")
    ).is_err());
    round_trip::<generated::RepeatedMixedFilterNotString>(serde_json::json!({ "key": "status" }));
    assert!(serde_json::from_value::<generated::RepeatedMixedFilterNotString>(
        serde_json::json!("scalar")
    ).is_err());
}
"##,
    )
    .expect("scratch main");

    let output = std::process::Command::new("cargo")
        .args(["run", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/object-union-allof-smoke"),
        )
        .output()
        .expect("cargo run");
    assert!(
        output.status.success(),
        "generated object/union allOf round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn allof_restating_a_union_up_to_annotations_resolves_to_that_union() {
    let analysis = analyze();
    let SchemaType::Reference { target } = &analysis.schemas["RestatedFilter"].schema_type else {
        panic!(
            "RestatedFilter should alias FilterNode, got {:?}",
            analysis.schemas["RestatedFilter"].schema_type
        );
    };
    assert_eq!(target, "FilterNode");
}

#[test]
fn restated_unions_retain_owner_properties_and_required_names() {
    for keyword in ["anyOf", "oneOf"] {
        let union = json!({keyword: [
            {"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]},
            {"type": "object", "properties": {"b": {"type": "integer"}}, "required": ["b"]}
        ]});
        let reference = json!({"$ref": "#/components/schemas/Union"});
        for members in [
            json!([reference, reference]),
            json!([reference, union]),
            json!([union, reference]),
            json!([union, union]),
        ] {
            for (declares_property, requires_property) in
                [(true, true), (true, false), (false, true)]
            {
                let mut owner = json!({"allOf": members});
                if declares_property {
                    owner["properties"] = json!({"owner_field": {"type": "string"}});
                }
                if requires_property {
                    owner["required"] = json!(["owner_field"]);
                }
                let spec = json!({
                    "openapi": "3.1.0",
                    "info": {"title": "restated union owner", "version": "1"},
                    "paths": {},
                    "components": {"schemas": {"Union": union, "Owner": owner}}
                });
                let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
                let SchemaType::Object {
                    properties,
                    required,
                    variant,
                    ..
                } = &analysis.schemas["Owner"].schema_type
                else {
                    panic!(
                        "owner fields were dropped for {owner}: {:?}",
                        analysis.schemas["Owner"].schema_type
                    );
                };
                assert!(variant.is_some(), "owner must retain its union for {owner}");
                assert_eq!(
                    required.contains("owner_field"),
                    requires_property,
                    "{owner}"
                );
                let property = &properties["owner_field"];
                if declares_property {
                    assert!(
                        matches!(
                            &property.schema_type,
                            SchemaType::Primitive { rust_type, .. } if rust_type == "String"
                        ),
                        "declared owner field must keep its type for {owner}"
                    );
                } else {
                    assert!(property.synthesized_required, "{owner}");
                }
            }
        }
    }
}

#[test]
fn restated_unions_preserve_owner_assertions_for_mixed_branches() {
    for keyword in ["anyOf", "oneOf"] {
        let union = json!({keyword: [
            {"type": "object", "properties": {"key": {"type": "string"}}, "required": ["key"]},
            {"type": "string"}
        ]});
        assert!(jsonschema::is_valid(&union, &json!("scalar")));
        for mut owner in [
            json!({"type": "object"}),
            json!({"not": {"type": "string"}}),
        ] {
            owner["allOf"] = json!([union, union]);
            assert!(!jsonschema::is_valid(&owner, &json!("scalar")));
            owner["allOf"] = json!([
                {"$ref": "#/components/schemas/Union"},
                {"$ref": "#/components/schemas/Union"}
            ]);
            let spec = json!({
                "openapi": "3.1.0",
                "info": {"title": "restated union owner type", "version": "1"},
                "paths": {},
                "components": {"schemas": {
                    "Union": union,
                    "Owner": owner
                }}
            });
            let analysis = SchemaAnalyzer::new(spec).unwrap().analyze().unwrap();
            assert!(
                matches!(
                    &analysis.schemas["Owner"].schema_type,
                    SchemaType::Object {
                        variant: Some(_),
                        ..
                    }
                ),
                "owner must still require an object: {:?}",
                analysis.schemas["Owner"].schema_type
            );
        }
    }
}

#[test]
fn multiple_union_intersections_are_reported_instead_of_silently_dropped() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "multiple unions", "version": "1" },
        "paths": {},
        "components": {
            "schemas": {
                "ImpossibleToProject": {
                    "allOf": [
                        { "oneOf": [{ "type": "string" }, { "type": "integer" }] },
                        { "anyOf": [{ "type": "boolean" }, { "type": "array" }] },
                        { "type": "object", "properties": { "id": { "type": "string" } } }
                    ]
                }
            }
        }
    });
    let error = SchemaAnalyzer::new(spec)
        .expect("multiple union spec should parse")
        .analyze()
        .expect_err("multiple allOf unions need an explicit diagnostic");
    assert!(
        error
            .to_string()
            .contains("intersects multiple union members"),
        "unexpected error: {error}"
    );
}

fn assert_distinct_unions_are_rejected(first: Value, second: Value, witness: Value) {
    assert!(jsonschema::is_valid(&first, &witness));
    assert!(!jsonschema::is_valid(&second, &witness));
    for members in [json!([first, second]), json!([second, first])] {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "distinct unions", "version": "1" },
            "paths": {},
            "components": { "schemas": { "Intersection": { "allOf": members } } }
        });
        let error = SchemaAnalyzer::new(spec)
            .unwrap()
            .analyze()
            .expect_err("different union assertions must not be deduplicated");
        assert!(
            error
                .to_string()
                .contains("intersects multiple union members"),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn union_restatements_preserve_dependent_required_data() {
    for keyword in ["anyOf", "oneOf"] {
        let first = json!({keyword: [
            {"type": "object", "dependentRequired": {"description": ["kind"]}},
            {"type": "string"}
        ]});
        let second = json!({keyword: [
            {"type": "object", "dependentRequired": {"description": ["value"]}},
            {"type": "string"}
        ]});
        assert_distinct_unions_are_rejected(
            first,
            second,
            json!({"description": "x", "kind": "y"}),
        );
    }
}

#[test]
fn union_restatements_preserve_object_type_for_mixed_branches() {
    for keyword in ["anyOf", "oneOf"] {
        let first = json!({keyword: [{"type": "object"}, {"type": "string"}]});
        let second = json!({"type": "object", keyword: [
            {"type": "object"}, {"type": "string"}
        ]});
        assert_distinct_unions_are_rejected(first, second, json!("scalar"));
    }
}
