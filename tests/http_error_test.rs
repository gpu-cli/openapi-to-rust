use openapi_to_rust::http_error::{HttpError, HttpResult};

#[test]
fn test_http_error_creation() {
    // Test serialization error
    let ser_error = HttpError::serialization_error("failed to serialize");
    assert!(matches!(ser_error, HttpError::Serialization(_)));

    // Test deserialization error
    let deser_error = HttpError::deserialization_error("failed to deserialize");
    assert!(matches!(deser_error, HttpError::Deserialization(_)));

    // Test auth error
    let auth_error = HttpError::Auth("invalid token".to_string());
    assert!(matches!(auth_error, HttpError::Auth(_)));

    // Test timeout error
    let timeout_error = HttpError::Timeout;
    assert!(matches!(timeout_error, HttpError::Timeout));

    // Test config error
    let config_error = HttpError::Config("invalid config".to_string());
    assert!(matches!(config_error, HttpError::Config(_)));

    // Test other error
    let other_error = HttpError::Other("something went wrong".to_string());
    assert!(matches!(other_error, HttpError::Other(_)));
}

#[test]
fn test_http_error_from_status() {
    // Test creating HTTP error from status
    let error = HttpError::from_status(404, "Not Found", Some("Resource not found".to_string()));

    match error {
        HttpError::Http {
            status,
            message,
            body,
        } => {
            assert_eq!(status, 404);
            assert_eq!(message, "Not Found");
            assert_eq!(body, Some("Resource not found".to_string()));
        }
        _ => panic!("Expected HTTP error"),
    }

    // Test without body
    let error = HttpError::from_status(500, "Internal Server Error", None);
    match error {
        HttpError::Http {
            status,
            message,
            body,
        } => {
            assert_eq!(status, 500);
            assert_eq!(message, "Internal Server Error");
            assert_eq!(body, None);
        }
        _ => panic!("Expected HTTP error"),
    }
}

#[test]
fn test_error_classification() {
    // Test client errors (4xx)
    let client_error = HttpError::from_status(400, "Bad Request", None);
    assert!(client_error.is_client_error());
    assert!(!client_error.is_server_error());

    let not_found = HttpError::from_status(404, "Not Found", None);
    assert!(not_found.is_client_error());
    assert!(!not_found.is_server_error());

    let forbidden = HttpError::from_status(403, "Forbidden", None);
    assert!(forbidden.is_client_error());
    assert!(!forbidden.is_server_error());

    // Test server errors (5xx)
    let server_error = HttpError::from_status(500, "Internal Server Error", None);
    assert!(!server_error.is_client_error());
    assert!(server_error.is_server_error());

    let bad_gateway = HttpError::from_status(502, "Bad Gateway", None);
    assert!(!bad_gateway.is_client_error());
    assert!(bad_gateway.is_server_error());

    // Test non-HTTP errors
    let timeout = HttpError::Timeout;
    assert!(!timeout.is_client_error());
    assert!(!timeout.is_server_error());

    let auth = HttpError::Auth("invalid".to_string());
    assert!(!auth.is_client_error());
    assert!(!auth.is_server_error());
}

#[test]
fn test_retryable_errors() {
    // Test timeout is retryable
    let timeout = HttpError::Timeout;
    assert!(timeout.is_retryable());

    // Test rate limit (429) is retryable
    let rate_limit = HttpError::from_status(429, "Too Many Requests", None);
    assert!(rate_limit.is_retryable());

    // Test server errors that are retryable
    let internal_error = HttpError::from_status(500, "Internal Server Error", None);
    assert!(internal_error.is_retryable());

    let bad_gateway = HttpError::from_status(502, "Bad Gateway", None);
    assert!(bad_gateway.is_retryable());

    let service_unavailable = HttpError::from_status(503, "Service Unavailable", None);
    assert!(service_unavailable.is_retryable());

    let gateway_timeout = HttpError::from_status(504, "Gateway Timeout", None);
    assert!(gateway_timeout.is_retryable());

    // Test client errors that are NOT retryable
    let bad_request = HttpError::from_status(400, "Bad Request", None);
    assert!(!bad_request.is_retryable());

    let not_found = HttpError::from_status(404, "Not Found", None);
    assert!(!not_found.is_retryable());

    let forbidden = HttpError::from_status(403, "Forbidden", None);
    assert!(!forbidden.is_retryable());

    // Test other errors are NOT retryable
    let auth_error = HttpError::Auth("invalid token".to_string());
    assert!(!auth_error.is_retryable());

    let serialization_error = HttpError::Serialization("failed".to_string());
    assert!(!serialization_error.is_retryable());

    let deserialization_error = HttpError::Deserialization("failed".to_string());
    assert!(!deserialization_error.is_retryable());

    let config_error = HttpError::Config("invalid".to_string());
    assert!(!config_error.is_retryable());
}

#[test]
fn test_serialization_errors() {
    // Test serialization error helper
    let error = HttpError::serialization_error("JSON serialization failed");
    match error {
        HttpError::Serialization(msg) => {
            assert_eq!(msg, "JSON serialization failed");
        }
        _ => panic!("Expected Serialization error"),
    }

    // Test deserialization error helper
    let error = HttpError::deserialization_error("JSON deserialization failed");
    match error {
        HttpError::Deserialization(msg) => {
            assert_eq!(msg, "JSON deserialization failed");
        }
        _ => panic!("Expected Deserialization error"),
    }

    // Test with Display trait
    let error = HttpError::serialization_error(std::fmt::Error);
    match error {
        HttpError::Serialization(msg) => {
            assert!(!msg.is_empty());
        }
        _ => panic!("Expected Serialization error"),
    }
}

#[test]
fn test_error_display() {
    // Test that errors display correctly
    let http_error = HttpError::from_status(404, "Not Found", None);
    let display_str = format!("{}", http_error);
    assert!(display_str.contains("404"));
    assert!(display_str.contains("Not Found"));

    let timeout_error = HttpError::Timeout;
    let display_str = format!("{}", timeout_error);
    assert!(display_str.contains("timeout"));

    let auth_error = HttpError::Auth("invalid token".to_string());
    let display_str = format!("{}", auth_error);
    assert!(display_str.contains("Authentication"));
    assert!(display_str.contains("invalid token"));

    let serialization_error = HttpError::Serialization("failed to serialize".to_string());
    let display_str = format!("{}", serialization_error);
    assert!(display_str.contains("serialize"));

    let deserialization_error = HttpError::Deserialization("failed to deserialize".to_string());
    let display_str = format!("{}", deserialization_error);
    assert!(display_str.contains("deserialize"));

    let config_error = HttpError::Config("invalid config".to_string());
    let display_str = format!("{}", config_error);
    assert!(display_str.contains("Configuration"));
}

#[test]
fn test_generated_error_code() {
    use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalysis};
    use std::path::PathBuf;

    // Create a minimal generator config
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);

    // Create empty analysis
    use std::collections::BTreeMap;
    let mut analysis = SchemaAnalysis {
        schemas: BTreeMap::new(),
        dependencies: openapi_to_rust::analysis::DependencyGraph::new(),
        patterns: openapi_to_rust::analysis::DetectedPatterns {
            tagged_enum_schemas: std::collections::HashSet::new(),
            untagged_enum_schemas: std::collections::HashSet::new(),
            type_mappings: BTreeMap::new(),
        },
        operations: BTreeMap::new(),
        used_type_features: Default::default(),
    };

    // Generate HTTP client code which includes error types
    let result = generator.generate_all(&mut analysis);
    assert!(result.is_ok());

    let generation_result = result.expect("generation should succeed");

    // Find the client.rs file
    let client_file = generation_result
        .files
        .iter()
        .find(|f| f.path.to_str().unwrap_or("").contains("client.rs"));

    assert!(client_file.is_some(), "Should generate client.rs file");

    let client_content = &client_file.expect("client file").content;

    // Transport-level error machinery
    assert!(
        client_content.contains("pub enum HttpError"),
        "Generated code should contain HttpError enum"
    );
    assert!(
        client_content.contains("Network(#[from] reqwest::Error)"),
        "Generated code should contain Network variant"
    );
    assert!(
        client_content.contains("Serialization(String)"),
        "Generated code should contain Serialization variant"
    );
    assert!(
        client_content.contains("Auth(String)"),
        "Generated code should contain Auth variant"
    );
    assert!(
        client_content.contains("Timeout"),
        "Generated code should contain Timeout variant"
    );
    assert!(
        client_content.contains("Config(String)"),
        "Generated code should contain Config variant"
    );
    assert!(
        client_content.contains("Other(String)"),
        "Generated code should contain Other variant"
    );

    // New typed-response envelope (replaces the old HttpError::Http variant
    // and from_status helper — see issue #8).
    assert!(
        client_content.contains("pub struct ApiError"),
        "Generated code should contain ApiError struct"
    );
    assert!(
        client_content.contains("pub enum ApiOpError"),
        "Generated code should contain ApiOpError enum"
    );
    assert!(
        client_content.contains("Transport(#[from] HttpError)"),
        "ApiOpError should have Transport variant carrying HttpError"
    );

    // Helpers on the new envelope
    assert!(
        client_content.contains("pub fn serialization_error"),
        "Generated code should contain serialization_error method"
    );
    assert!(
        client_content.contains("pub fn is_client_error"),
        "Generated code should contain is_client_error method"
    );
    assert!(
        client_content.contains("pub fn is_server_error"),
        "Generated code should contain is_server_error method"
    );
    assert!(
        client_content.contains("pub fn is_retryable"),
        "Generated code should contain is_retryable method"
    );

    // Verify HttpResult type alias
    assert!(
        client_content.contains("pub type HttpResult<T> = Result<T, HttpError>"),
        "Generated code should contain HttpResult type alias"
    );

    // Verify thiserror is used
    assert!(
        client_content.contains("use thiserror::Error"),
        "Generated code should import thiserror::Error"
    );
}

#[test]
fn test_http_result_type() {
    // Test that HttpResult works correctly
    let success: HttpResult<i32> = Ok(42);
    assert!(success.is_ok());
    if let Ok(val) = success {
        assert_eq!(val, 42);
    }

    let failure: HttpResult<i32> = Err(HttpError::Timeout);
    assert!(failure.is_err());
    match failure {
        Err(HttpError::Timeout) => {}
        _ => panic!("Expected Timeout error"),
    }

    // Test with HTTP error
    let http_error: HttpResult<String> = Err(HttpError::from_status(404, "Not Found", None));
    assert!(http_error.is_err());
    match http_error {
        Err(e) => {
            assert!(e.is_client_error());
            assert!(!e.is_retryable());
        }
        _ => panic!("Expected error"),
    }
}
