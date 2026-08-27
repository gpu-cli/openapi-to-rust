use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn load_fixture(relative_path: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json") => serde_json::from_str(&source)
            .unwrap_or_else(|error| panic!("failed to parse {} as JSON: {error}", path.display())),
        Some("yaml" | "yml") => serde_yaml::from_str(&source)
            .unwrap_or_else(|error| panic!("failed to parse {} as YAML: {error}", path.display())),
        extension => panic!(
            "unsupported fixture extension {extension:?}: {}",
            path.display()
        ),
    }
}

fn component_schema<'a>(fixture: &'a Value, name: &str) -> &'a Value {
    fixture
        .pointer(&format!("/components/schemas/{name}"))
        .unwrap_or_else(|| panic!("missing #/components/schemas/{name}"))
}

fn assert_draft4_meta_valid(pointer: &str, schema: &Value) {
    if let Err(error) = jsonschema::draft4::meta::validate(schema) {
        panic!("{pointer} is not Draft 4 meta-schema valid: {error}");
    }
}

fn assert_draft202012_meta_valid(pointer: &str, schema: &Value) {
    if let Err(error) = jsonschema::draft202012::meta::validate(schema) {
        panic!("{pointer} is not Draft 2020-12 meta-schema valid: {error}");
    }
}

fn assert_draft202012_instances(
    pointer: &str,
    schema: &Value,
    accepted: &[Value],
    rejected: &[Value],
) {
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(schema)
        .unwrap_or_else(|error| panic!("failed to compile {pointer}: {error}"));
    for instance in accepted {
        assert!(
            validator.is_valid(instance),
            "{pointer} unexpectedly rejected {instance}"
        );
    }
    for instance in rejected {
        assert!(
            !validator.is_valid(instance),
            "{pointer} unexpectedly accepted {instance}"
        );
    }
}

#[test]
fn cloudflare_empty_required_repair_is_draft4_valid() {
    let fixture = load_fixture("specs/cloudflare.yaml");
    for name in ["ErrorData", "pagination_info"] {
        let pointer = format!("#/components/schemas/{name}");
        let schema = component_schema(&fixture, name);
        assert_eq!(
            schema.get("required"),
            None,
            "{pointer} properties are intentionally optional"
        );
        assert_draft4_meta_valid(&pointer, schema);
    }
}

#[test]
fn coda_required_members_are_declared_and_draft4_valid() {
    let fixture = load_fixture("specs/coda.yaml");
    let cases = [
        ("Table", &["viewId"][..], &[][..]),
        (
            "DocAnalyticsMetrics",
            &[
                "aiCreditsChat,",
                "aiCreditsBlock,",
                "aiCreditsColumn,",
                "aiCreditsAssistant,",
                "aiCreditsReviewer,",
                "aiCredits,",
            ][..],
            &[
                "aiCreditsChat",
                "aiCreditsBlock",
                "aiCreditsColumn",
                "aiCreditsAssistant",
                "aiCreditsReviewer",
                "aiCredits",
            ][..],
        ),
        (
            "IngestionBatchExecution",
            &["errorMessage", "ingestionStatuses"][..],
            &["ingestionStatusCounts"][..],
        ),
        ("IngestionExecutionAttempt", &["message"][..], &[][..]),
        ("IngestionParentItem", &["creationTimestamp"][..], &[][..]),
    ];

    for (name, absent_required, present_required) in cases {
        let pointer = format!("#/components/schemas/{name}");
        let schema = component_schema(&fixture, name);
        let required = schema["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{pointer}/required is not an array"));
        let properties = schema["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{pointer}/properties is not an object"));

        for member in absent_required {
            assert!(
                !required.iter().any(|value| value.as_str() == Some(member)),
                "{pointer}/required still contains stale member {member:?}"
            );
        }
        for member in present_required {
            assert!(
                required.iter().any(|value| value.as_str() == Some(member)),
                "{pointer}/required lost intended member {member:?}"
            );
        }
        for member in required {
            let member = member
                .as_str()
                .unwrap_or_else(|| panic!("{pointer}/required contains a non-string"));
            assert!(
                properties.contains_key(member),
                "{pointer}/required member {member:?} has no declared property while additionalProperties is false"
            );
        }
        assert_draft4_meta_valid(&pointer, schema);
    }
}

#[test]
fn discord_enum_components_match_upstream_and_are_2020_12_valid() {
    let fixture = load_fixture("specs/discord.json");

    // Synchronized on 2026-08-27 from Discord's generated preview specification:
    // https://github.com/discord/discord-api-spec/blob/main/specs/openapi_preview.json
    // No immutable upstream revision was recorded with the original corpus import.
    let cases = [
        (
            "ApplicationCommandHandler",
            json!([
                {
                    "title": "APP_HANDLER",
                    "description": "The app handles the interaction using an interaction token",
                    "const": 1
                },
                {
                    "title": "DISCORD_LAUNCH_ACTIVITY",
                    "description": "Discord handles the interaction by launching an Activity and sending a follow-up message without coordinating with the app",
                    "const": 2
                }
            ]),
            vec![json!(1), json!(2)],
            vec![json!(0), json!("1")],
        ),
        (
            "EntitlementOwnerTypes",
            json!([
                {
                    "title": "GUILD",
                    "description": "A guild subscription",
                    "const": 1
                },
                {
                    "title": "USER",
                    "description": "A user subscription",
                    "const": 2
                }
            ]),
            vec![json!(1), json!(2)],
            vec![json!(0), json!(3)],
        ),
        (
            "NameplatePalette",
            json!([
                {
                    "title": "CRIMSON",
                    "description": "Crimson color palette",
                    "const": "crimson"
                },
                {
                    "title": "BERRY",
                    "description": "Berry color palette",
                    "const": "berry"
                },
                {
                    "title": "SKY",
                    "description": "Sky color palette",
                    "const": "sky"
                },
                {
                    "title": "TEAL",
                    "description": "Teal color palette",
                    "const": "teal"
                },
                {
                    "title": "FOREST",
                    "description": "Forest color palette",
                    "const": "forest"
                },
                {
                    "title": "BUBBLE_GUM",
                    "description": "Bubble gum color palette",
                    "const": "bubble_gum"
                },
                {
                    "title": "VIOLET",
                    "description": "Violet color palette",
                    "const": "violet"
                },
                {
                    "title": "COBALT",
                    "description": "Cobalt color palette",
                    "const": "cobalt"
                },
                {
                    "title": "CLOVER",
                    "description": "Clover color palette",
                    "const": "clover"
                },
                {
                    "title": "LEMON",
                    "description": "Lemon color palette",
                    "const": "lemon"
                },
                {
                    "title": "WHITE",
                    "description": "White color palette",
                    "const": "white"
                },
                {
                    "title": "BLACK",
                    "description": "Black color palette",
                    "const": "black"
                }
            ]),
            [
                "crimson",
                "berry",
                "sky",
                "teal",
                "forest",
                "bubble_gum",
                "violet",
                "cobalt",
                "clover",
                "lemon",
                "white",
                "black",
            ]
            .into_iter()
            .map(Value::from)
            .collect(),
            vec![json!("orange"), json!(1)],
        ),
        (
            "PollLayoutTypes",
            json!([
                {
                    "title": "DEFAULT",
                    "description": "The, uhm, default layout type.",
                    "const": 1
                }
            ]),
            vec![json!(1)],
            vec![json!(0), json!(2)],
        ),
    ];

    for (name, expected_one_of, accepted, rejected) in cases {
        let pointer = format!("#/components/schemas/{name}");
        let schema = component_schema(&fixture, name);
        assert_eq!(
            schema.get("oneOf"),
            Some(&expected_one_of),
            "{pointer}/oneOf differs from the upstream definition"
        );
        assert_draft202012_meta_valid(&pointer, schema);
        assert_draft202012_instances(&pointer, schema, &accepted, &rejected);
    }
}

fn collect_null_type_arrays(value: &Value, pointer: &str, found: &mut Vec<String>) {
    match value {
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                collect_null_type_arrays(child, &format!("{pointer}/{index}"), found);
            }
        }
        Value::Object(object) => {
            if object
                .get("type")
                .and_then(Value::as_array)
                .is_some_and(|types| types.iter().any(Value::is_null))
            {
                found.push(format!("{pointer}/type"));
            }
            for (name, child) in object {
                collect_null_type_arrays(child, &format!("{pointer}/{name}"), found);
            }
        }
        _ => {}
    }
}

#[test]
fn imagekit_nullable_type_arrays_are_strings_and_2020_12_valid() {
    let fixture = load_fixture("specs/imagekit.yaml");
    let expected = [
        (
            "/components/schemas/UpdateFileRequest/oneOf/0/properties/tags/type",
            json!(["array", "null"]),
        ),
        (
            "/components/schemas/UpdateFileRequest/oneOf/0/properties/customCoordinates/type",
            json!(["string", "null"]),
        ),
        (
            "/components/schemas/FileDetails/properties/tags/type",
            json!(["array", "null"]),
        ),
        (
            "/components/schemas/FileDetails/properties/AITags/type",
            json!(["array", "null"]),
        ),
        (
            "/components/schemas/FileDetails/properties/customCoordinates/type",
            json!(["string", "null"]),
        ),
        (
            "/components/schemas/Upload/properties/tags/type",
            json!(["array", "null"]),
        ),
        (
            "/components/schemas/Upload/properties/AITags/type",
            json!(["array", "null"]),
        ),
        (
            "/components/schemas/Upload/properties/customCoordinates/type",
            json!(["string", "null"]),
        ),
    ];
    for (pointer, expected_types) in expected {
        assert_eq!(
            fixture.pointer(pointer),
            Some(&expected_types),
            "#{pointer} must contain JSON Schema type-name strings"
        );
    }

    let mut null_type_arrays = Vec::new();
    collect_null_type_arrays(&fixture, "", &mut null_type_arrays);
    assert!(
        null_type_arrays.is_empty(),
        "ImageKit type arrays still contain YAML null scalars: {null_type_arrays:?}"
    );

    for name in ["UpdateFileRequest", "FileDetails", "Upload"] {
        let pointer = format!("#/components/schemas/{name}");
        assert_draft202012_meta_valid(&pointer, component_schema(&fixture, name));
    }
}
