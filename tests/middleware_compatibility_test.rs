use openapi_generator::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use std::path::PathBuf;

#[test]
fn test_generated_http_client_imports_are_correct() {
    let spec = std::fs::read_to_string("tests/fixtures/simple.json")
        .expect("Failed to read simple.json fixture");
    let spec_value: serde_json::Value = serde_json::from_str(&spec).expect("Failed to parse JSON");

    let mut analyzer = SchemaAnalyzer::new(spec_value).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("tests/fixtures/simple.json"),
        output_dir: PathBuf::from("target/test_output"),
        module_name: "test_module".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Should use super::types, not crate::types
    assert!(
        client_code.contains("use super::types::*;"),
        "Generated client should use 'super::types' not 'crate::types'. Generated code:\n{}",
        client_code
    );
    assert!(
        !client_code.contains("use crate::types::*;"),
        "Generated client should NOT use 'crate::types'. Generated code:\n{}",
        client_code
    );
}

#[test]
fn test_generated_client_uses_middleware_compatible_json() {
    let spec = std::fs::read_to_string("tests/fixtures/post_with_body.json")
        .expect("Failed to read post_with_body.json fixture");
    let spec_value: serde_json::Value = serde_json::from_str(&spec).expect("Failed to parse JSON");

    let mut analyzer = SchemaAnalyzer::new(spec_value).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("tests/fixtures/post_with_body.json"),
        output_dir: PathBuf::from("target/test_output"),
        module_name: "test_module".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Should NOT use .json() which doesn't exist on middleware RequestBuilder
    assert!(
        !client_code.contains(".json(&request)"),
        "Should not use .json() on middleware RequestBuilder. Generated code:\n{}",
        client_code
    );

    // Should use .body() with serialization
    assert!(
        client_code.contains(".body(") && client_code.contains("serde_json::"),
        "Should use .body() with serde_json for request serialization. Generated code:\n{}",
        client_code
    );
}

#[test]
fn test_post_operation_has_content_type_header() {
    let spec = std::fs::read_to_string("tests/fixtures/post_with_body.json")
        .expect("Failed to read post_with_body.json fixture");
    let spec_value: serde_json::Value = serde_json::from_str(&spec).expect("Failed to parse JSON");

    let mut analyzer = SchemaAnalyzer::new(spec_value).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("tests/fixtures/post_with_body.json"),
        output_dir: PathBuf::from("target/test_output"),
        module_name: "test_module".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Should set content-type header for JSON
    assert!(
        client_code.contains("application/json"),
        "Should set application/json content-type for JSON requests. Generated code:\n{}",
        client_code
    );
}

#[test]
fn test_body_serialization_uses_serde_json() {
    let spec = std::fs::read_to_string("tests/fixtures/post_with_body.json")
        .expect("Failed to read post_with_body.json fixture");
    let spec_value: serde_json::Value = serde_json::from_str(&spec).expect("Failed to parse JSON");

    let mut analyzer = SchemaAnalyzer::new(spec_value).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("tests/fixtures/post_with_body.json"),
        output_dir: PathBuf::from("target/test_output"),
        module_name: "test_module".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Should use serde_json::to_vec for serialization
    assert!(
        client_code.contains("serde_json::to_vec(&request)"),
        "Should use serde_json::to_vec for request serialization. Generated code:\n{}",
        client_code
    );

    // Should handle serialization errors properly using the helper method
    assert!(
        client_code.contains("HttpError::serialization_error"),
        "Should handle serialization errors using helper method. Generated code:\n{}",
        client_code
    );
}
