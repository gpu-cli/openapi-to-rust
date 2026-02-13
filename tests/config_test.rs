use openapi_generator::ConfigFile;
use std::io::Write;
use tempfile::NamedTempFile;

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
