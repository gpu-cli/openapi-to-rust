use openapi_to_rust::analysis::SchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn discriminator_spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": { "title": "discriminator allowed values", "version": "1" },
        "paths": {},
        "components": { "schemas": {
            "Track": {
                "type": "object",
                "required": ["type", "id"],
                "properties": {
                    "type": { "type": "string", "enum": ["track"] },
                    "id": { "type": "string" }
                }
            },
            "EpisodeBase": {
                "type": "object",
                "required": ["type", "id"],
                "properties": {
                    "type": { "type": "string", "const": "episode" },
                    "id": { "type": "string" }
                }
            },
            "Episode": {
                "allOf": [
                    { "$ref": "#/components/schemas/EpisodeBase" },
                    {
                        "type": "object",
                        "required": ["show"],
                        "properties": { "show": { "type": "string" } }
                    }
                ]
            },
            "Media": {
                "oneOf": [
                    { "$ref": "#/components/schemas/Track" },
                    { "$ref": "#/components/schemas/Episode" }
                ],
                "discriminator": { "propertyName": "type" }
            },
            "StreamOutput": {
                "type": "object",
                "required": ["type", "data"],
                "properties": {
                    "type": { "type": "string", "enum": ["stdout", "stderr"] },
                    "data": { "type": "string" }
                }
            },
            "RichOutputKinds": {
                "type": "string",
                "enum": ["display_data", "execute_result"]
            },
            "DisplayOrExecuteOutput": {
                "type": "object",
                "required": ["type", "data"],
                "properties": {
                    "type": {
                        "allOf": [{ "$ref": "#/components/schemas/RichOutputKinds" }]
                    },
                    "data": { "type": "object" }
                }
            },
            "InterpreterOutput": {
                "oneOf": [
                    { "$ref": "#/components/schemas/StreamOutput" },
                    { "$ref": "#/components/schemas/DisplayOrExecuteOutput" }
                ],
                "discriminator": { "propertyName": "type" }
            },
            "ConversationChannelType": {
                "type": "string",
                "enum": ["phone_call", "sms_chat"]
            },
            "ScheduledPhoneCallEventResponse": {
                "type": "object",
                "required": ["channel", "target"],
                "properties": {
                    "channel": { "$ref": "#/components/schemas/ConversationChannelType" },
                    "target": { "type": "string" }
                }
            },
            "ScheduledSmsEventResponse": {
                "type": "object",
                "required": ["channel", "target", "text"],
                "properties": {
                    "channel": { "$ref": "#/components/schemas/ConversationChannelType" },
                    "target": { "type": "string" },
                    "text": { "type": "string" }
                }
            },
            "ScheduledEventResponse": {
                "anyOf": [
                    { "$ref": "#/components/schemas/ScheduledPhoneCallEventResponse" },
                    { "$ref": "#/components/schemas/ScheduledSmsEventResponse" }
                ],
                "discriminator": { "propertyName": "channel" }
            },
            "BroadMappedKind": {
                "type": "string",
                "enum": ["alpha", "beta"]
            },
            "BroadMappedObject": {
                "type": "object",
                "required": ["kind", "id"],
                "properties": {
                    "kind": { "$ref": "#/components/schemas/BroadMappedKind" },
                    "id": { "type": "string" }
                }
            },
            "MappedAlpha": {
                "allOf": [
                    { "$ref": "#/components/schemas/BroadMappedObject" },
                    {
                        "type": "object",
                        "required": ["alpha"],
                        "properties": {
                            "kind": { "type": "string", "const": "alpha" },
                            "alpha": { "type": "string" }
                        }
                    }
                ]
            },
            "MappedBeta": {
                "allOf": [
                    { "$ref": "#/components/schemas/BroadMappedObject" },
                    {
                        "type": "object",
                        "required": ["beta"],
                        "properties": {
                            "kind": { "type": "string", "const": "beta" },
                            "beta": { "type": "string" }
                        }
                    }
                ]
            },
            "ConflictingMappedUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/MappedAlpha" },
                    { "$ref": "#/components/schemas/MappedBeta" }
                ],
                "discriminator": {
                    "propertyName": "kind",
                    "mapping": {
                        "alpha": "#/components/schemas/MappedBeta",
                        "beta": "#/components/schemas/MappedAlpha",
                        "bogus": "#/components/schemas/MappedAlpha"
                    }
                }
            },
            "ScalarCarrier": {
                "oneOf": [
                    { "type": "string" },
                    { "type": "number" },
                    { "type": "boolean" }
                ]
            },
            "ObjectCarrier": {
                "type": "object",
                "required": ["@type", "value"],
                "properties": {
                    "@type": { "type": "string", "const": "object" },
                    "value": { "type": "string" }
                }
            },
            "ScalarOrTaggedObject": {
                "oneOf": [
                    { "$ref": "#/components/schemas/ScalarCarrier" },
                    { "$ref": "#/components/schemas/ObjectCarrier" }
                ],
                "discriminator": { "propertyName": "@type" }
            },
            "StainlessAlpha": {
                "type": "object",
                "required": ["type", "alpha"],
                "properties": {
                    "type": {
                        "const": "stainless.alpha",
                        "x-stainless-const": true
                    },
                    "alpha": { "type": "string" }
                }
            },
            "StainlessBeta": {
                "type": "object",
                "required": ["type", "beta"],
                "properties": {
                    "type": {
                        "const": "stainless.beta",
                        "x-stainless-const": true
                    },
                    "beta": { "type": "string" }
                }
            },
            "StainlessEvent": {
                "anyOf": [
                    { "$ref": "#/components/schemas/StainlessAlpha" },
                    { "$ref": "#/components/schemas/StainlessBeta" }
                ],
                "discriminator": { "propertyName": "type" }
            },
            "VersionedPreview": {
                "type": "object",
                "required": ["type", "preview"],
                "properties": {
                    "type": {
                        "type": "string",
                        "enum": ["preview", "preview_v2"],
                        "default": "preview",
                        "x-stainless-const": true
                    },
                    "preview": { "type": "string" }
                }
            },
            "VersionedOther": {
                "type": "object",
                "required": ["type", "other"],
                "properties": {
                    "type": { "type": "string", "const": "other" },
                    "other": { "type": "string" }
                }
            },
            "StainlessVersionedEvent": {
                "anyOf": [
                    { "$ref": "#/components/schemas/VersionedPreview" },
                    { "$ref": "#/components/schemas/VersionedOther" }
                ],
                "discriminator": { "propertyName": "type" }
            }
        } }
    })
}

fn union_values(analysis: &openapi_to_rust::SchemaAnalysis, name: &str) -> Vec<Vec<String>> {
    let SchemaType::DiscriminatedUnion { variants, .. } = &analysis.schemas[name].schema_type
    else {
        panic!("{name} should be a discriminated union");
    };
    variants
        .iter()
        .map(|variant| variant.discriminator_values.clone())
        .collect()
}

fn preferred_union_values(
    analysis: &openapi_to_rust::SchemaAnalysis,
    name: &str,
) -> Vec<Vec<String>> {
    let SchemaType::DiscriminatedUnion { variants, .. } = &analysis.schemas[name].schema_type
    else {
        panic!("{name} should be a discriminated union");
    };
    variants
        .iter()
        .map(|variant| variant.preferred_discriminator_values.clone())
        .collect()
}

#[test]
fn discriminator_values_follow_refs_compositions_and_multi_value_enums() {
    let analysis = SchemaAnalyzer::new(discriminator_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");

    assert_eq!(
        union_values(&analysis, "Media"),
        vec![vec!["track"], vec!["episode"]]
    );
    assert_eq!(
        union_values(&analysis, "InterpreterOutput"),
        vec![
            vec![String::from("stdout"), String::from("stderr")],
            vec![String::from("display_data"), String::from("execute_result")]
        ]
    );
    assert_eq!(
        union_values(&analysis, "ScheduledEventResponse"),
        vec![
            vec![String::from("phone_call"), String::from("sms_chat")],
            vec![String::from("phone_call"), String::from("sms_chat")]
        ]
    );
    assert_eq!(
        preferred_union_values(&analysis, "ScheduledEventResponse"),
        vec![
            vec![String::from("phone_call")],
            vec![String::from("sms_chat")]
        ]
    );
    assert_eq!(
        union_values(&analysis, "ConflictingMappedUnion"),
        vec![vec![String::from("alpha")], vec![String::from("beta")]]
    );
    assert!(matches!(
        &analysis.schemas["ScalarOrTaggedObject"].schema_type,
        SchemaType::Union { .. }
    ));
    assert_eq!(
        union_values(&analysis, "StainlessEvent"),
        vec![vec!["stainless.alpha"], vec!["stainless.beta"]]
    );
    assert_eq!(
        union_values(&analysis, "StainlessVersionedEvent"),
        vec![vec!["preview", "preview_v2"], vec!["other"]]
    );
}

#[test]
fn generated_unions_preserve_every_allowed_discriminator_value() {
    let mut analysis = SchemaAnalyzer::new(discriminator_spec())
        .expect("spec should parse")
        .analyze()
        .expect("spec should analyze");
    let mut generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("spec should generate");
    generated.push_str(
        r#"
#[cfg(test)]
mod discriminator_allowed_value_roundtrip {
    use super::{
        ConflictingMappedUnion, InterpreterOutput, Media, ScalarOrTaggedObject,
        ScheduledEventResponse, StainlessEvent, StainlessVersionedEvent,
    };
    use serde::{Serialize, de::DeserializeOwned};

    fn exact<T>(input: serde_json::Value)
    where
        T: DeserializeOwned + Serialize,
    {
        let hydrated: T = serde_json::from_value(input.clone()).expect("hydrate allowed tag");
        assert_eq!(serde_json::to_value(hydrated).unwrap(), input);
    }

    #[test]
    fn every_allowed_tag_round_trips_exactly() {
        exact::<Media>(serde_json::json!({"type": "track", "id": "t"}));
        exact::<Media>(serde_json::json!({"type": "episode", "id": "e", "show": "s"}));

        for tag in ["stdout", "stderr"] {
            exact::<InterpreterOutput>(serde_json::json!({"type": tag, "data": "line"}));
        }
        for tag in ["display_data", "execute_result"] {
            exact::<InterpreterOutput>(serde_json::json!({"type": tag, "data": {}}));
        }

        exact::<ScheduledEventResponse>(serde_json::json!({
            "channel": "phone_call", "target": "voice"
        }));
        exact::<ScheduledEventResponse>(serde_json::json!({
            "channel": "sms_chat", "target": "text", "text": "hello"
        }));
        // The source schema permits either shared enum value in either branch.
        // If the preferred SMS branch does not fit, dispatch falls back to the
        // phone payload while preserving the original tag.
        exact::<ScheduledEventResponse>(serde_json::json!({
            "channel": "sms_chat", "target": "voice-without-text"
        }));

        exact::<ConflictingMappedUnion>(serde_json::json!({
            "kind": "alpha", "id": "a", "alpha": "payload"
        }));
        exact::<ConflictingMappedUnion>(serde_json::json!({
            "kind": "beta", "id": "b", "beta": "payload"
        }));

        exact::<ScalarOrTaggedObject>(serde_json::json!(12.5));
        exact::<ScalarOrTaggedObject>(serde_json::json!({
            "@type": "object", "value": "payload"
        }));
        exact::<StainlessEvent>(serde_json::json!({
            "type": "stainless.alpha", "alpha": "payload"
        }));
        exact::<StainlessEvent>(serde_json::json!({
            "type": "stainless.beta", "beta": "payload"
        }));
        exact::<StainlessVersionedEvent>(serde_json::json!({
            "type": "preview", "preview": "first"
        }));
        exact::<StainlessVersionedEvent>(serde_json::json!({
            "type": "preview_v2", "preview": "second"
        }));
    }
}
"#,
    );

    let temp = tempfile::TempDir::new().expect("temporary scratch crate");
    fs::write(
        temp.path().join("Cargo.toml"),
        r#"[package]
name = "discriminator-allowed-values-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
"#,
    )
    .expect("write scratch manifest");
    fs::create_dir(temp.path().join("src")).expect("create scratch source");
    fs::write(temp.path().join("src/lib.rs"), generated).expect("write generated source");

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--offline"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/discriminator-allowed-values-smoke"),
        )
        .output()
        .expect("run generated round-trip test");
    assert!(
        output.status.success(),
        "generated discriminator round trip failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
