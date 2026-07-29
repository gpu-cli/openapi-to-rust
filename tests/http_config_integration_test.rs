use openapi_to_rust::{AuthConfig, ConfigFile};
use std::io::Write;
use tempfile::NamedTempFile;

fn write_config(toml_content: &str) -> NamedTempFile {
    let spec_path = std::fs::canonicalize("tests/fixtures/simple.json")
        .expect("fixture path should canonicalize");
    let toml_content = toml_content.replace(
        "tests/fixtures/simple.json",
        &spec_path.display().to_string(),
    );
    let mut temp_file = NamedTempFile::new().expect("Failed to create temp file");
    temp_file
        .write_all(toml_content.as_bytes())
        .expect("Failed to write to temp file");
    temp_file
}

#[test]
fn test_http_config_toml_to_runtime() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true
enable_sse_client = false
enable_specta = false

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 30
max_response_body_bytes = 4194304

[[http_client.headers]]
name = "content-type"
value = "application/json"

[[http_client.headers]]
name = "user-agent"
value = "rust-client"
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify HTTP client config was converted correctly
    let http_config = generator_config
        .http_client_config
        .expect("HTTP client config should be present");
    assert_eq!(
        http_config.base_url,
        Some("https://api.example.com".to_string())
    );
    assert_eq!(http_config.timeout_seconds, Some(30));
    assert_eq!(http_config.max_response_body_bytes, Some(4_194_304));
    assert_eq!(http_config.default_headers.len(), 2);
    assert_eq!(
        http_config.default_headers.get("content-type"),
        Some(&"application/json".to_string())
    );
    assert_eq!(
        http_config.default_headers.get("user-agent"),
        Some(&"rust-client".to_string())
    );
}

#[test]
fn test_retry_config_conversion() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true

[http_client.retry]
max_retries = 5
initial_delay_ms = 1000
max_delay_ms = 30000
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify retry config was converted correctly
    let retry_config = generator_config
        .retry_config
        .expect("Retry config should be present");
    assert_eq!(retry_config.max_retries, 5);
    assert_eq!(retry_config.initial_delay_ms, 1000);
    assert_eq!(retry_config.max_delay_ms, 30000);
}

#[test]
fn test_auth_config_conversion_bearer() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true

[http_client.auth]
type = "Bearer"
header_name = "Authorization"
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify auth config was converted correctly
    let auth_config = generator_config
        .auth_config
        .expect("Auth config should be present");
    match auth_config {
        AuthConfig::Bearer { header_name } => {
            assert_eq!(header_name, "Authorization");
        }
        _ => panic!("Expected Bearer auth config"),
    }
}

#[test]
fn test_auth_config_conversion_api_key() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true

[http_client.auth]
type = "ApiKey"
header_name = "X-API-Key"
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify auth config was converted correctly
    let auth_config = generator_config
        .auth_config
        .expect("Auth config should be present");
    match auth_config {
        AuthConfig::ApiKey { header_name } => {
            assert_eq!(header_name, "X-API-Key");
        }
        _ => panic!("Expected ApiKey auth config"),
    }
}

#[test]
fn test_auth_config_conversion_custom() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true

[http_client.auth]
type = "Custom"
header_name = "X-Custom-Auth"
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify auth config was converted correctly
    let auth_config = generator_config
        .auth_config
        .expect("Auth config should be present");
    match auth_config {
        AuthConfig::Custom {
            header_name,
            header_value_prefix,
        } => {
            assert_eq!(header_name, "X-Custom-Auth");
            assert_eq!(header_value_prefix, None);
        }
        _ => panic!("Expected Custom auth config"),
    }
}

#[test]
fn test_default_values() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify defaults when HTTP client section is omitted
    assert!(generator_config.http_client_config.is_none());
    assert!(generator_config.retry_config.is_none());
    assert!(generator_config.auth_config.is_none());
    // Tracing should default to true
    assert!(generator_config.tracing_enabled);
}

#[test]
fn test_tracing_config() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true

[http_client.tracing]
enabled = false
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify tracing can be disabled
    assert!(!generator_config.tracing_enabled);
}

#[test]
fn test_complete_http_client_config() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true
enable_sse_client = false

[http_client]
base_url = "https://api.openai.com/v1"
timeout_seconds = 60

[http_client.retry]
max_retries = 3
initial_delay_ms = 500
max_delay_ms = 16000

[http_client.tracing]
enabled = true

[http_client.auth]
type = "Bearer"
header_name = "Authorization"

[[http_client.headers]]
name = "content-type"
value = "application/json"
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify all HTTP client config components
    assert!(generator_config.http_client_config.is_some());
    assert!(generator_config.retry_config.is_some());
    assert!(generator_config.auth_config.is_some());
    assert!(generator_config.tracing_enabled);

    let http_config = generator_config.http_client_config.unwrap();
    assert_eq!(
        http_config.base_url,
        Some("https://api.openai.com/v1".to_string())
    );
    assert_eq!(http_config.timeout_seconds, Some(60));

    let retry_config = generator_config.retry_config.unwrap();
    assert_eq!(retry_config.max_retries, 3);
    assert_eq!(retry_config.initial_delay_ms, 500);
    assert_eq!(retry_config.max_delay_ms, 16000);
}

#[test]
fn test_retry_config_defaults() {
    let toml_content = r#"
[generator]
spec_path = "tests/fixtures/simple.json"
output_dir = "out"
module_name = "test"

[features]
enable_async_client = true

[http_client.retry]
"#;

    let temp_file = write_config(toml_content);

    let config_file = ConfigFile::load(temp_file.path()).expect("Failed to load config");
    let generator_config = config_file.into_generator_config();

    // Verify default values for retry config
    let retry_config = generator_config
        .retry_config
        .expect("Retry config should be present");
    assert_eq!(retry_config.max_retries, 3); // default
    assert_eq!(retry_config.initial_delay_ms, 500); // default
    assert_eq!(retry_config.max_delay_ms, 16000); // default
}
