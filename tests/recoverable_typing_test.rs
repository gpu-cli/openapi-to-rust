//! Regression tests for schemas that used to generate `serde_json::Value`
//! despite carrying enough information to type (issue #62 follow-up).
//!
//! Each case here was found by the untyped census (`--report-untyped`) across
//! the 57-spec corpus and traced back to a real document. The tests assert two
//! things per pattern: the generated Rust type, and that the census no longer
//! reports the field as recoverable — a fix that types the field but leaves the
//! census claiming otherwise would make the corpus numbers lie.
//!
//! The negative cases matter as much as the positive ones. Several fixes narrow
//! a schema to a type the spec does not strictly require, and the tests pin the
//! line: a union that genuinely has no single Rust type must stay a union, and
//! an unconstrained value must stay `serde_json::Value`.

use openapi_to_rust::analysis::{SchemaAnalysis, UntypedVerdict};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};

fn analyze(spec: Value) -> SchemaAnalysis {
    SchemaAnalyzer::new(spec)
        .expect("spec parses")
        .analyze()
        .expect("spec analyzes")
}

fn generate(spec: Value) -> String {
    let mut analysis = analyze(spec.clone());
    CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("code generates")
}

/// Generated source plus the census verdict, which every case checks together.
fn generate_and_census(spec: Value) -> (String, Vec<String>) {
    let generated = generate(spec.clone());
    let recoverable = analyze(spec)
        .untyped_fields()
        .into_iter()
        .filter(|finding| finding.reason.verdict() == UntypedVerdict::Recoverable)
        .map(|finding| format!("{} ({:?})", finding.context, finding.reason))
        .collect();
    (generated, recoverable)
}

fn spec_with_schemas(schemas: Value) -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "typing", "version": "1.0.0" },
        "components": { "schemas": schemas }
    })
}

fn assert_types(spec: Value, expected: &[&str]) {
    let (generated, recoverable) = generate_and_census(spec);
    for want in expected {
        assert!(
            generated.contains(want),
            "expected `{want}` in generated output:\n{generated}"
        );
    }
    assert!(
        recoverable.is_empty(),
        "census still reports recoverable untyped fields: {recoverable:?}"
    );
}

#[test]
fn odata_nullable_reference_union_becomes_option() {
    // `anyOf: [$ref, {type: object, nullable: true}]` is how OData spells "that
    // type, or null" — Microsoft Graph emits it for every navigation property,
    // 2,127 times across the corpus. Read literally it is "that type or any
    // object", which has no single Rust type.
    assert_types(
        spec_with_schemas(json!({
            "User": { "type": "object", "additionalProperties": false,
                      "properties": { "id": { "type": "string" } } },
            "Member": { "type": "object", "additionalProperties": false, "properties": {
                "user": { "anyOf": [
                    { "$ref": "#/components/schemas/User" },
                    { "type": "object", "nullable": true }
                ]}
            }}
        })),
        &["pub user: Option<User>"],
    );
}

#[test]
fn a_nullable_branch_that_constrains_something_stays_a_union() {
    // The narrowing above applies only to an *empty* nullable object. A branch
    // with properties of its own is a real alternative, and collapsing it would
    // silently drop a shape the API can return.
    let (generated, _) = generate_and_census(spec_with_schemas(json!({
        "User": { "type": "object", "additionalProperties": false,
                  "properties": { "id": { "type": "string" } } },
        "Member": { "type": "object", "additionalProperties": false, "properties": {
            "user": { "anyOf": [
                { "$ref": "#/components/schemas/User" },
                { "type": "object", "nullable": true,
                  "properties": { "deletedAt": { "type": "string" } } }
            ]}
        }}
    })));

    assert!(
        !generated.contains("pub user: Option<User>"),
        "a branch with its own properties must not collapse to the other branch:\n{generated}"
    );
}

#[test]
fn open_string_enum_union_becomes_an_extensible_enum() {
    // Anthropic's `AnthropicBeta`: a named set of values, plus any other string.
    assert_types(
        spec_with_schemas(json!({
            "Beta": { "anyOf": [
                { "type": "string" },
                { "type": "string", "enum": ["computer-use-2024-10-22", "pdfs-2024-09-25"] }
            ]},
            "Holder": { "type": "object", "additionalProperties": false,
                        "properties": { "beta": { "$ref": "#/components/schemas/Beta" } } }
        })),
        &["pub enum Beta", "ComputerUse20241022", "Custom(String)"],
    );
}

#[test]
fn single_member_composition_keeps_its_member_type() {
    // `allOf: [{type: string}]` is how countless 3.0 specs hang a description
    // off a scalar. Box does it for `sequence_id`.
    assert_types(
        spec_with_schemas(json!({
            "Thing": { "type": "object", "additionalProperties": false, "properties": {
                "sequence_id": { "allOf": [{ "type": "string", "description": "a numeric id" }] }
            }}
        })),
        &["pub sequence_id: Option<String>"],
    );
}

#[test]
fn composition_of_a_reference_and_an_extension_is_hoisted_to_a_struct() {
    // Asana's `AllocationResponse.assignee`: a `$ref` merged with an inline
    // object that adds fields. The merge already worked; the merged object then
    // sat in a field position, which the generator cannot render.
    let (generated, recoverable) = generate_and_census(spec_with_schemas(json!({
        "UserCompact": { "type": "object", "additionalProperties": false,
                         "properties": { "gid": { "type": "string" } } },
        "Allocation": { "type": "object", "additionalProperties": false, "properties": {
            "assignee": { "allOf": [
                { "$ref": "#/components/schemas/UserCompact" },
                { "type": "object", "properties": { "name": { "type": "string" } } }
            ]}
        }}
    })));

    assert!(
        !generated.contains("pub assignee: Option<serde_json::Value>"),
        "the merged composition must keep a type:\n{generated}"
    );
    assert!(
        generated.contains("pub gid") && generated.contains("pub name"),
        "the hoisted type must carry both sides of the merge:\n{generated}"
    );
    assert!(recoverable.is_empty(), "{recoverable:?}");
}

#[test]
fn an_inline_object_property_is_hoisted_to_a_named_struct() {
    assert_types(
        spec_with_schemas(json!({
            "Thing": { "type": "object", "additionalProperties": false, "properties": {
                "nested": { "type": "object", "additionalProperties": false,
                            "properties": { "count": { "type": "integer" } } }
            }}
        })),
        &["pub nested: Option<ThingNested>", "pub struct ThingNested"],
    );
}

#[test]
fn a_composition_inside_array_items_keeps_its_type() {
    // Asana's `WebhookRequest.filters[]`. Array elements went through an
    // analyzer that had no `allOf` case at all.
    assert_types(
        spec_with_schemas(json!({
            "Filter": { "type": "object", "additionalProperties": false,
                        "properties": { "action": { "type": "string" } } },
            "Webhook": { "type": "object", "additionalProperties": false, "properties": {
                "filters": { "type": "array", "items": { "allOf": [
                    { "$ref": "#/components/schemas/Filter" },
                    { "type": "object", "properties": { "fields": { "type": "string" } } }
                ]}}
            }}
        })),
        &["pub filters: Option<Vec<"],
    );
}

#[test]
fn a_deep_pointer_reference_resolves_to_the_node_it_names() {
    // PagerDuty references a parameter's schema and a single member of another
    // schema's composition. Only `#/components/schemas/<name>` used to resolve.
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "typing", "version": "1.0.0" },
        "components": {
            "parameters": {
                "audit_method_type": {
                    "name": "type", "in": "query",
                    "schema": { "type": "string", "enum": ["web_session", "api_token"] }
                }
            },
            "schemas": {
                "Method": { "type": "object", "additionalProperties": false, "properties": {
                    "type": { "$ref": "#/components/parameters/audit_method_type/schema" }
                }}
            }
        }
    });

    let (generated, recoverable) = generate_and_census(spec);
    assert!(
        !generated.contains("pub r#type: Option<serde_json::Value>")
            && !generated.contains("pub type_: Option<serde_json::Value>"),
        "a resolvable pointer must not degrade to an untyped value:\n{generated}"
    );
    assert!(recoverable.is_empty(), "{recoverable:?}");
}

#[test]
fn a_null_typed_property_becomes_the_unit_type() {
    // Discord's `GuildRoleTagsResponse.premium_subscriber` is `{"type": "null"}`:
    // present, always null. serde reads and writes `()` as exactly that.
    assert_types(
        spec_with_schemas(json!({
            "Tags": { "type": "object", "additionalProperties": false, "properties": {
                "premium_subscriber": { "type": "null" }
            }}
        })),
        &["pub premium_subscriber: Option<()>"],
    );
}

#[test]
fn an_empty_union_falls_back_to_the_declared_type() {
    // Discord ships `{type: integer, format: int32, oneOf: []}`. An empty branch
    // list constrains nothing, so the `type` keyword is the whole schema.
    assert_types(
        spec_with_schemas(json!({
            "OwnerTypes": { "type": "integer", "format": "int32", "oneOf": [] },
            "Palette": { "type": "string", "oneOf": [] },
            "Holder": { "type": "object", "additionalProperties": false, "properties": {
                "owner": { "$ref": "#/components/schemas/OwnerTypes" },
                "palette": { "$ref": "#/components/schemas/Palette" }
            }}
        })),
        &["pub type OwnerTypes = i32", "pub type Palette = String"],
    );
}

#[test]
fn a_single_branch_union_is_that_branch() {
    // gcore wraps a referenced error schema in `anyOf: [...]` to attach an
    // example to it.
    assert_types(
        spec_with_schemas(json!({
            "ValidationError": { "type": "object", "additionalProperties": false,
                                 "properties": { "detail": { "type": "string" } } },
            "ChangePasswordError": { "anyOf": [
                { "allOf": [{ "$ref": "#/components/schemas/ValidationError" }] }
            ]},
            "Holder": { "type": "object", "additionalProperties": false, "properties": {
                "error": { "$ref": "#/components/schemas/ChangePasswordError" }
            }}
        })),
        &["pub error: Option<ChangePasswordError>"],
    );
}

#[test]
fn union_branches_differing_only_in_constraints_share_one_type() {
    // Runway declares a URI field as three `string` branches with different
    // `pattern`s and lengths. Every value is still a String.
    assert_types(
        spec_with_schemas(json!({
            "Media": { "type": "object", "additionalProperties": false, "properties": {
                "uri": { "anyOf": [
                    { "type": "string", "pattern": "^https://.*", "maxLength": 2048 },
                    { "type": "string", "pattern": "^runway://.*", "maxLength": 5000 },
                    { "type": "string", "pattern": "^data:.*" }
                ]}
            }}
        })),
        // Hoisted under a name, which resolves to the shared type.
        &["pub uri: Option<MediaUri>", "pub type MediaUri = String"],
    );
}

#[test]
fn union_branches_with_different_formats_fall_back_to_the_wire_type() {
    // gcore: `ipv4 | ipv6 | ipv4network | ipv6network`. The typed-scalar
    // refinements disagree — `Ipv4Addr` is not `Ipv6Addr` — but every branch is
    // a string, and that much holds for every value.
    assert_types(
        spec_with_schemas(json!({
            "Profile": { "type": "object", "additionalProperties": false, "properties": {
                "ip_address": { "anyOf": [
                    { "type": "string", "format": "ipv4" },
                    { "type": "string", "format": "ipv6" },
                    { "type": "null" }
                ]}
            }}
        })),
        &["String"],
    );
}

#[test]
fn a_union_that_only_alternates_requiredness_is_the_object_it_describes() {
    // Cloudflare: "one of commit_hash or branch must be present". Rust cannot
    // express the alternation, but the object is right there in `properties`.
    assert_types(
        spec_with_schemas(json!({
            "Build": {
                "anyOf": [{ "required": ["commit_hash"] }, { "required": ["branch"] }],
                "properties": {
                    "branch": { "type": "string" },
                    "commit_hash": { "type": "string" }
                }
            }
        })),
        &[
            "pub struct Build",
            "pub branch: Option<String>",
            "pub commit_hash: Option<String>",
        ],
    );
}

#[test]
fn union_branches_that_are_deep_pointers_are_expanded() {
    // PagerDuty builds a request body from three pointers into a response's
    // `oneOf`. Every branch resolves, but a union of unresolvable references
    // had nothing to build variants from.
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "typing", "version": "1.0.0" },
        "components": {
            "responses": { "CacheData": {
                "description": "cache data",
                "content": { "application/json": { "schema": {
                    "oneOf": [{ "type": "string" }, { "type": "number" }]
                }}}
            }},
            "schemas": {
                "PutRequest": { "type": "object", "oneOf": [
                    { "$ref": "#/components/responses/CacheData/content/application~1json/schema/oneOf/0" },
                    { "$ref": "#/components/responses/CacheData/content/application~1json/schema/oneOf/1" }
                ]},
                "Holder": { "type": "object", "additionalProperties": false, "properties": {
                    "data": { "$ref": "#/components/schemas/PutRequest" }
                }}
            }
        }
    });

    let (generated, recoverable) = generate_and_census(spec);
    assert!(
        generated.contains("pub enum PutRequest")
            && generated.contains("String(String)")
            && generated.contains("Number(f64)"),
        "pointer branches must expand into union variants:\n{generated}"
    );
    assert!(recoverable.is_empty(), "{recoverable:?}");
}

#[test]
fn a_pointer_into_a_composition_resolves_to_that_member() {
    // The other pointer form PagerDuty uses: one member of another schema's
    // `allOf`, addressed by index.
    let (generated, recoverable) = generate_and_census(spec_with_schemas(json!({
        "Tag": { "allOf": [
            { "type": "object", "additionalProperties": false,
              "properties": { "label": { "type": "string" } } }
        ]},
        "Action": { "type": "object", "additionalProperties": false, "properties": {
            "base": { "$ref": "#/components/schemas/Tag/allOf/0" }
        }}
    })));

    assert!(
        !generated.contains("pub base: Option<serde_json::Value>"),
        "a pointer into a composition must resolve:\n{generated}"
    );
    assert!(recoverable.is_empty(), "{recoverable:?}");
}

#[test]
fn an_unresolvable_reference_still_degrades_rather_than_failing() {
    // Pointer resolution must not turn a bad reference into a hard error: the
    // rest of the document still generates, and the census reports the field so
    // it can be found.
    let analysis = analyze(spec_with_schemas(json!({
        "Thing": { "type": "object", "additionalProperties": false, "properties": {
            "external": { "$ref": "https://example.com/schemas/Other.json" },
            "missing": { "$ref": "#/components/schemas/DoesNotExist/allOf/7" }
        }}
    })));

    let reasons = analysis
        .untyped_fields()
        .into_iter()
        .map(|finding| finding.reason)
        .collect::<Vec<_>>();
    assert!(
        reasons
            .iter()
            .all(|reason| *reason == openapi_to_rust::analysis::UntypedReason::UnresolvedReference),
        "unresolvable refs must be reported as such, got {reasons:?}"
    );
}

#[test]
fn a_nullable_reference_branch_is_not_treated_as_null() {
    // `{$ref: ..., nullable: true}` constrains its target; only an *empty*
    // nullable object is the null marker. Collapsing this would pick the wrong
    // branch of the union.
    let (generated, _) = generate_and_census(spec_with_schemas(json!({
        "A": { "type": "object", "additionalProperties": false,
               "properties": { "a": { "type": "string" } } },
        "B": { "type": "object", "additionalProperties": false,
               "properties": { "b": { "type": "string" } } },
        "Holder": { "type": "object", "additionalProperties": false, "properties": {
            "either": { "anyOf": [
                { "$ref": "#/components/schemas/A" },
                { "$ref": "#/components/schemas/B", "nullable": true }
            ]}
        }}
    })));

    assert!(
        !generated.contains("pub either: Option<A>"),
        "a nullable $ref branch is a real alternative, not a null marker:\n{generated}"
    );
}

#[test]
fn items_true_parses_as_an_unconstrained_array() {
    // The other half of boolean `items`: `true` accepts anything, so the array
    // is untyped but the document still parses.
    let (generated, recoverable) = generate_and_census(spec_with_schemas(json!({
        "Thing": { "type": "object", "additionalProperties": false, "properties": {
            "anything": { "type": "array", "items": true }
        }}
    })));

    assert!(
        generated.contains("pub anything: Option<Vec<serde_json::Value>>"),
        "`items: true` must parse and stay unconstrained:\n{generated}"
    );
    assert!(recoverable.is_empty(), "{recoverable:?}");
}

#[test]
fn a_genuinely_unconstrained_value_stays_untyped() {
    // The counterweight to every narrowing above: when the schema says "any
    // JSON", `serde_json::Value` is the right answer and the census must call
    // it faithful rather than recoverable.
    let (generated, recoverable) = generate_and_census(spec_with_schemas(json!({
        "Thing": { "type": "object", "additionalProperties": false, "properties": {
            "metadata": {},
            "payload": { "type": "object" }
        }}
    })));

    assert!(
        generated.contains("pub metadata: Option<serde_json::Value>"),
        "an empty schema must stay untyped:\n{generated}"
    );
    assert!(recoverable.is_empty(), "{recoverable:?}");
}
