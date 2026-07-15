use openapi_to_rust::config::GeneratorSection;
use openapi_to_rust::type_mapping::{ByteStrategy, DateStrategy};
use openapi_to_rust::{CodeGenerator, ConfigFile, GeneratorConfig, SchemaAnalyzer, TypeMapper};
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

#[test]
fn test_valid_config_minimal() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(spec_file, r#"{{"openapi": "3.0.0"}}"#).unwrap();
    let spec_path = spec_file.path();

    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]
enable_sse_client = false
enable_async_client = false
enable_specta = false"#,
        spec_path.display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    let result = ConfigFile::load(config_file.path());
    assert!(
        result.is_ok(),
        "Minimal valid config should load successfully: {:?}",
        result.err()
    );

    let config = result.unwrap();
    assert_eq!(config.generator.module_name, "types");
    assert!(!config.features.enable_sse_client);
    assert!(!config.features.enable_async_client);
    assert!(!config.features.enable_specta);
}

#[test]
fn test_valid_config_full() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(spec_file, r#"{{"openapi": "3.0.0"}}"#).unwrap();
    let spec_path = spec_file.path();

    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_sse_client = true
enable_async_client = true
enable_specta = true

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 60

[http_client.retry]
max_retries = 5
initial_delay_ms = 1000
max_delay_ms = 30000

[http_client.tracing]
enabled = true

[http_client.auth]
type = "Bearer"
header_name = "Authorization"

[[http_client.headers]]
name = "content-type"
value = "application/json"

[[http_client.headers]]
name = "user-agent"
value = "test-client"

[nullable_overrides]
"Response.error" = true

[type_mappings]
"DateTime" = "chrono::DateTime<chrono::Utc>""#,
        spec_path.display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    let result = ConfigFile::load(config_file.path());
    assert!(
        result.is_ok(),
        "Full valid config should load successfully: {:?}",
        result.err()
    );

    let config = result.unwrap();
    assert_eq!(config.generator.module_name, "api");
    assert!(config.features.enable_sse_client);
    assert!(config.features.enable_async_client);
    assert!(config.features.enable_specta);

    let http_client = config.http_client.as_ref().unwrap();
    assert_eq!(
        http_client.base_url.as_ref().unwrap(),
        "https://api.example.com"
    );
    assert_eq!(http_client.timeout_seconds, Some(60));

    let retry = http_client.retry.as_ref().unwrap();
    assert_eq!(retry.max_retries, 5);
    assert_eq!(retry.initial_delay_ms, 1000);
    assert_eq!(retry.max_delay_ms, 30000);

    let auth = http_client.auth.as_ref().unwrap();
    assert_eq!(auth.auth_type, "Bearer");
    assert_eq!(auth.header_name, "Authorization");

    assert_eq!(http_client.headers.len(), 2);
    assert_eq!(http_client.headers[0].name, "content-type");
    assert_eq!(http_client.headers[0].value, "application/json");

    assert_eq!(config.nullable_overrides.len(), 1);
    assert_eq!(config.nullable_overrides.get("Response.error"), Some(&true));

    assert_eq!(config.type_mappings.len(), 1);
}

#[test]
fn test_invalid_spec_path() {
    let config_content = r#"[generator]
spec_path = "nonexistent.json"
output_dir = "src/generated"
module_name = "types"

[features]
enable_sse_client = false"#;

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    let result = ConfigFile::load(config_file.path());
    assert!(result.is_err(), "Should fail with nonexistent spec file");

    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("OpenAPI spec file not found"),
        "Error should mention file not found: {}",
        err_msg
    );
}

#[test]
fn test_invalid_auth_type() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(spec_file, r#"{{"openapi": "3.0.0"}}"#).unwrap();
    let spec_path = spec_file.path();

    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]
enable_sse_client = false

[http_client.auth]
type = "InvalidType"
header_name = "Authorization""#,
        spec_path.display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    let result = ConfigFile::load(config_file.path());
    assert!(result.is_err(), "Should fail with invalid auth type");

    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("Invalid auth type"),
        "Error should mention invalid auth type: {}",
        err_msg
    );
}

#[test]
fn test_invalid_retry_config() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(spec_file, r#"{{"openapi": "3.0.0"}}"#).unwrap();
    let spec_path = spec_file.path();

    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]
enable_sse_client = false

[http_client.retry]
max_retries = 99
initial_delay_ms = 500
max_delay_ms = 16000"#,
        spec_path.display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    let result = ConfigFile::load(config_file.path());
    assert!(
        result.is_err(),
        "Should fail with out-of-range retry config"
    );

    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("max_retries must be between 0 and 10"),
        "Error should mention retry validation: {}",
        err_msg
    );
}

#[test]
fn test_invalid_timeout() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(spec_file, r#"{{"openapi": "3.0.0"}}"#).unwrap();
    let spec_path = spec_file.path();

    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]
enable_sse_client = false

[http_client]
timeout_seconds = 9999"#,
        spec_path.display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    let result = ConfigFile::load(config_file.path());
    assert!(result.is_err(), "Should fail with invalid timeout");

    let err = result.unwrap_err();
    let err_msg = format!("{}", err);
    assert!(
        err_msg.contains("timeout_seconds must be between 1 and 3600"),
        "Error should mention timeout validation: {}",
        err_msg
    );
}

#[test]
fn test_cli_validate_command() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(spec_file, r#"{{"openapi": "3.0.0"}}"#).unwrap();
    let spec_path = spec_file.path();

    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]
enable_sse_client = true"#,
        spec_path.display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    // Test that config can be loaded and validated
    let result = ConfigFile::load(config_file.path());
    assert!(
        result.is_ok(),
        "CLI validate should pass for valid config: {:?}",
        result.err()
    );
}

#[test]
fn test_cli_generate_command_config_parsing() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(
        spec_file,
        r#"{{"openapi": "3.0.0", "info": {{"title": "Test", "version": "1.0.0"}}, "paths": {{}}}}"#
    )
    .unwrap();
    let spec_path = spec_file.path();

    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]
enable_sse_client = false
enable_async_client = false"#,
        spec_path.display()
    );

    let mut config_file = NamedTempFile::new().unwrap();
    writeln!(config_file, "{}", config_content).unwrap();

    // Test that config can be converted to GeneratorConfig
    let config = ConfigFile::load(config_file.path()).unwrap();
    let generator_config = config.into_generator_config();

    assert_eq!(generator_config.module_name, "types");
    assert!(!generator_config.enable_sse_client);
    assert!(!generator_config.enable_async_client);
}

fn config_with_spec(spec_path: &std::path::Path, extra: &str) -> String {
    format!(
        r#"[generator]
spec_path = "{}"
output_dir = "generated"
module_name = "types"

{extra}

[features]
"#,
        spec_path.display()
    )
}

fn write_config(dir: &TempDir, content: &str) -> std::path::PathBuf {
    let path = dir.path().join("openapi-to-rust.toml");
    std::fs::write(&path, content).unwrap();
    path
}

#[test]
fn canonical_generator_types_parse_and_normalize() {
    let mut spec = NamedTempFile::new().unwrap();
    writeln!(spec, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        &config_with_spec(
            spec.path(),
            r#"[generator.types]
date_time = "string"
byte = "vec_u8"
unsigned = false"#,
        ),
    );

    let config = ConfigFile::load(&path).unwrap();
    assert_eq!(config.types.date_time, DateStrategy::String);
    assert_eq!(config.types.byte, ByteStrategy::VecU8);
    assert!(!config.types.unsigned);

    let direct: ConfigFile = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(direct.types.date_time, DateStrategy::String);
    assert_eq!(direct.types.byte, ByteStrategy::VecU8);
}

#[test]
fn generator_section_retains_standalone_serde_compatibility() {
    let section = GeneratorSection {
        spec_path: "openapi.yaml".into(),
        output_dir: "src/generated".into(),
        module_name: "api".into(),
        schema_extensions: vec!["overlay.yaml".into()],
    };

    let serialized = toml::to_string(&section).unwrap();
    let reparsed: GeneratorSection = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.spec_path, section.spec_path);
    assert_eq!(reparsed.output_dir, section.output_dir);
    assert_eq!(reparsed.module_name, section.module_name);
    assert_eq!(reparsed.schema_extensions, section.schema_extensions);
}

#[test]
fn legacy_root_types_alias_is_normalized_and_serializes_canonically() {
    let mut spec = NamedTempFile::new().unwrap();
    writeln!(spec, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        &config_with_spec(
            spec.path(),
            r#"[types]
date_time = "string"
unsigned = false"#,
        ),
    );

    let mut config = ConfigFile::load(&path).unwrap();
    assert_eq!(config.types.date_time, DateStrategy::String);
    assert!(!config.types.unsigned);
    config.types.byte = ByteStrategy::VecU8;
    assert_eq!(
        config.clone().into_generator_config().types.byte,
        ByteStrategy::VecU8
    );
    let serialized = toml::to_string(&config).unwrap();
    assert!(serialized.contains("[generator.types]"));
    assert!(!serialized.contains("\n[types]\n"));
    let reparsed: ConfigFile = toml::from_str(&serialized).unwrap();
    assert_eq!(reparsed.types.byte, ByteStrategy::VecU8);
}

#[test]
fn rejects_legacy_and_canonical_types_together() {
    let mut spec = NamedTempFile::new().unwrap();
    writeln!(spec, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();
    let body = config_with_spec(
        spec.path(),
        r#"[generator.types]
date_time = "chrono"

[types]
date_time = "string""#,
    );
    let err = ConfigFile::load(&write_config(&dir, &body)).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("both legacy [types] and canonical [generator.types]"));
    let direct_error = toml::from_str::<ConfigFile>(&body).unwrap_err().to_string();
    assert!(direct_error.contains("both legacy [types] and canonical [generator.types]"));
}

#[test]
fn rejects_obsolete_strategies_with_migration_guidance() {
    let mut spec = NamedTempFile::new().unwrap();
    writeln!(spec, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();
    let body = config_with_spec(
        spec.path(),
        r#"[generator.types.strategies]
date-time = "string"
byte = "vec_u8_base64""#,
    );
    let err = ConfigFile::load(&write_config(&dir, &body)).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("directly under [generator.types]"));
    assert!(message.contains("date_time"));
    assert!(message.contains("byte = \"base64\""));
}

#[test]
fn rejects_unknown_root_and_nested_fields() {
    let mut spec = NamedTempFile::new().unwrap();
    writeln!(spec, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();

    let root = format!(
        r#"mystery = true

[generator]
spec_path = "{}"
output_dir = "generated"
module_name = "types"

[features]
"#,
        spec.path().display()
    );
    let root_error = ConfigFile::load(&write_config(&dir, &root))
        .unwrap_err()
        .to_string();
    assert!(root_error.contains("unknown field `mystery`"));

    let nested = config_with_spec(spec.path(), "generator_typo = true");
    let nested_error = ConfigFile::load(&write_config(&dir, &nested))
        .unwrap_err()
        .to_string();
    assert!(nested_error.contains("unknown field `generator_typo`"));
}

#[test]
fn rejects_unknown_fields_in_fixed_nested_sections() {
    let mut spec = NamedTempFile::new().unwrap();
    writeln!(spec, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();

    for (extra, unknown) in [
        (
            r#"[generator.types.shape]
additional_properties_typo = true"#,
            "additional_properties_typo",
        ),
        (
            r#"[http_client.retry]
max_retries = 3
retry_typo = true"#,
            "retry_typo",
        ),
        (
            r#"[server]
framework = "axum"
server_typo = true"#,
            "server_typo",
        ),
    ] {
        let body = config_with_spec(spec.path(), extra);
        let error = ConfigFile::load(&write_config(&dir, &body))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(&format!("unknown field `{unknown}`")),
            "{error}"
        );
    }
}

#[test]
fn relative_generator_paths_are_config_file_relative() {
    let cwd = std::env::current_dir().unwrap();
    let workspace = tempfile::Builder::new()
        .prefix("config-relative-")
        .tempdir_in(&cwd)
        .unwrap();
    let config_dir = workspace.path().join("project/config");
    std::fs::create_dir_all(config_dir.join("overlays")).unwrap();
    std::fs::write(config_dir.join("openapi.json"), r#"{"openapi":"3.1.0"}"#).unwrap();
    std::fs::write(config_dir.join("overlays/extra.json"), "{}").unwrap();
    let config_path = config_dir.join("openapi-to-rust.toml");
    std::fs::write(
        &config_path,
        r#"[generator]
spec_path = "openapi.json"
output_dir = "../generated"
module_name = "types"
schema_extensions = ["overlays/extra.json"]

[features]
"#,
    )
    .unwrap();

    let relative_config = config_path.strip_prefix(&cwd).unwrap();
    let config = ConfigFile::load(relative_config).unwrap();
    assert_eq!(config.generator.spec_path, config_dir.join("openapi.json"));
    assert_eq!(config.generator.output_dir, config_dir.join("../generated"));
    assert_eq!(
        config.generator.schema_extensions,
        vec![config_dir.join("overlays/extra.json")]
    );
    assert!(!config.generator.output_dir.exists());
}

#[test]
fn checked_in_config_examples_load_from_any_working_directory() {
    for path in [
        "examples/server-openai-responses/openapi-to-rust.toml",
        "examples/server-anthropic-messages/openapi-to-rust.toml",
        "examples/type-mappings/openapi-to-rust.toml",
    ] {
        ConfigFile::load(std::path::Path::new(path))
            .unwrap_or_else(|error| panic!("{path} should load: {error}"));
    }
}

#[test]
fn documented_event_flow_value_preserves_start_delta_stop_configuration() {
    let mut spec = NamedTempFile::new().unwrap();
    writeln!(spec, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();
    let body = config_with_spec(
        spec.path(),
        r#"[streaming]

[[streaming.endpoints]]
operation_id = "streamEvents"
path = "/events"
event_union_type = "Event"

[streaming.endpoints.event_flow]
type = "StartDeltaStop"
start_events = ["start"]
delta_events = ["delta"]
stop_events = ["stop"]"#,
    );

    let runtime = ConfigFile::load(&write_config(&dir, &body))
        .unwrap()
        .into_generator_config();
    let endpoint = &runtime.streaming_config.unwrap().endpoints[0];
    assert!(matches!(
        endpoint.event_flow,
        openapi_to_rust::streaming::EventFlow::StartDeltaStop { .. }
    ));
}

#[test]
fn canonical_type_settings_materially_change_generated_output() {
    let mut spec_file = NamedTempFile::new().unwrap();
    writeln!(spec_file, r#"{{"openapi": "3.1.0"}}"#).unwrap();
    let dir = TempDir::new().unwrap();
    let path = write_config(
        &dir,
        &config_with_spec(
            spec_file.path(),
            r#"[generator.types]
date_time = "string"
unsigned = false
format_aliases = { custom_uuid = "uuid" }

[generator.types.shape]
additional_properties_typed = false"#,
        ),
    );
    let generator_config = ConfigFile::load(&path).unwrap().into_generator_config();
    let mapper = TypeMapper::new(generator_config.types.clone());
    let spec = serde_json::json!({
        "openapi": "3.1.0",
        "info": { "title": "config", "version": "1.0.0" },
        "paths": {},
        "components": { "schemas": {
            "Sample": {
                "type": "object",
                "required": ["at", "count", "id"],
                "properties": {
                    "at": { "type": "string", "format": "date-time" },
                    "count": { "type": "integer", "format": "uint64" },
                    "id": { "type": "string", "format": "custom_uuid" }
                }
            },
            "Bag": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            }
        }}
    });
    let mut analyzer = SchemaAnalyzer::with_type_mapper(spec, mapper).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let code = CodeGenerator::new(GeneratorConfig {
        module_name: "config".into(),
        ..Default::default()
    })
    .generate(&mut analysis)
    .unwrap();

    assert!(code.contains("pub at: String"));
    assert!(code.contains("pub count: i64"));
    assert!(code.contains("pub id: uuid::Uuid"));
    assert!(code.contains(
        "pub additional_properties: std::collections::BTreeMap<String, serde_json::Value>"
    ));
}
