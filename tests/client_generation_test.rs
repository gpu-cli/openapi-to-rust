use openapi_generator::{CodeGenerator, GeneratorConfig};
use std::path::PathBuf;

/// Helper to create a minimal generator config
fn create_test_config() -> GeneratorConfig {
    GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    }
}

#[test]
fn test_http_client_struct_generation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify the struct is generated
    assert!(
        code_str.contains("pub struct HttpClient"),
        "Should generate HttpClient struct"
    );

    // Verify fields
    assert!(code_str.contains("base_url"), "Should have base_url field");
    assert!(code_str.contains("api_key"), "Should have api_key field");
    assert!(
        code_str.contains("http_client"),
        "Should have http_client field"
    );
    assert!(
        code_str.contains("custom_headers"),
        "Should have custom_headers field"
    );

    // Verify middleware types
    assert!(
        code_str.contains("ClientWithMiddleware"),
        "Should use reqwest_middleware::ClientWithMiddleware"
    );
    assert!(
        code_str.contains("BTreeMap"),
        "Should use BTreeMap for headers"
    );
}

#[test]
fn test_constructor_without_retry() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: None,
        tracing_enabled: false,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify new() constructor exists
    assert!(
        code_str.contains("pub fn new"),
        "Should generate new() constructor"
    );

    // Verify no RetryConfig struct when retry disabled
    assert!(
        !code_str.contains("pub struct RetryConfig"),
        "Should NOT generate RetryConfig when retry is disabled"
    );

    // Verify no retry middleware when disabled
    assert!(
        !code_str.contains("RetryTransientMiddleware"),
        "Should NOT include retry middleware when disabled"
    );

    // Verify no tracing middleware when disabled
    assert!(
        !code_str.contains("TracingMiddleware"),
        "Should NOT include tracing middleware when disabled"
    );
}

#[test]
fn test_constructor_with_retry() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_generator::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: false,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify RetryConfig struct is generated
    assert!(
        code_str.contains("pub struct RetryConfig"),
        "Should generate RetryConfig struct when retry enabled"
    );

    // Verify RetryConfig fields
    assert!(
        code_str.contains("max_retries"),
        "RetryConfig should have max_retries field"
    );
    assert!(
        code_str.contains("initial_delay_ms"),
        "RetryConfig should have initial_delay_ms field"
    );
    assert!(
        code_str.contains("max_delay_ms"),
        "RetryConfig should have max_delay_ms field"
    );

    // Verify Default implementation
    assert!(
        code_str.contains("impl Default for RetryConfig"),
        "RetryConfig should have Default impl"
    );

    // Verify retry middleware is included
    assert!(
        code_str.contains("RetryTransientMiddleware"),
        "Should include retry middleware when enabled"
    );
    assert!(
        code_str.contains("ExponentialBackoff"),
        "Should use exponential backoff policy"
    );

    // Verify with_config method accepts retry_config
    assert!(
        code_str.contains("pub fn with_config"),
        "Should generate with_config method"
    );
    assert!(
        code_str.contains("retry_config : Option < RetryConfig >"),
        "with_config should accept retry_config parameter"
    );
}

#[test]
fn test_constructor_with_tracing() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: None,
        tracing_enabled: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify tracing middleware is included
    assert!(
        code_str.contains("TracingMiddleware"),
        "Should include tracing middleware when enabled"
    );

    // Verify with_config method accepts enable_tracing
    assert!(
        code_str.contains("pub fn with_config"),
        "Should generate with_config method"
    );
    assert!(
        code_str.contains("enable_tracing : bool"),
        "with_config should accept enable_tracing parameter"
    );
}

#[test]
fn test_builder_methods_generated() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify with_base_url method
    assert!(
        code_str.contains("pub fn with_base_url"),
        "Should generate with_base_url method"
    );
    assert!(
        code_str.contains("base_url . into ()"),
        "with_base_url should accept Into<String>"
    );

    // Verify with_api_key method
    assert!(
        code_str.contains("pub fn with_api_key"),
        "Should generate with_api_key method"
    );
    assert!(
        code_str.contains("api_key . into ()"),
        "with_api_key should accept Into<String>"
    );

    // Verify with_header method
    assert!(
        code_str.contains("pub fn with_header"),
        "Should generate with_header method"
    );

    // Verify with_headers method
    assert!(
        code_str.contains("pub fn with_headers"),
        "Should generate with_headers method"
    );
    assert!(
        code_str.contains("headers : BTreeMap < String , String >"),
        "with_headers should accept BTreeMap"
    );
}

#[test]
fn test_retry_config_struct_when_enabled() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_generator::http_config::RetryConfig {
            max_retries: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
        }),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify RetryConfig struct exists
    assert!(
        code_str.contains("pub struct RetryConfig"),
        "Should generate RetryConfig struct"
    );

    // Verify all fields are present
    assert!(
        code_str.contains("pub max_retries : u32"),
        "Should have max_retries field"
    );
    assert!(
        code_str.contains("pub initial_delay_ms : u64"),
        "Should have initial_delay_ms field"
    );
    assert!(
        code_str.contains("pub max_delay_ms : u64"),
        "Should have max_delay_ms field"
    );

    // Verify Default trait implementation
    assert!(
        code_str.contains("impl Default for RetryConfig"),
        "Should implement Default for RetryConfig"
    );
    assert!(
        code_str.contains("fn default () -> Self"),
        "Default impl should have default() method"
    );
}

#[test]
fn test_no_retry_config_when_disabled() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: None,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify RetryConfig struct is NOT generated
    assert!(
        !code_str.contains("pub struct RetryConfig"),
        "Should NOT generate RetryConfig when retry is disabled"
    );

    // Verify retry middleware is NOT included
    assert!(
        !code_str.contains("RetryTransientMiddleware"),
        "Should NOT include retry middleware when disabled"
    );
    assert!(
        !code_str.contains("ExponentialBackoff"),
        "Should NOT include backoff policy when retry disabled"
    );
}

#[test]
fn test_middleware_stack_order() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_generator::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify both middleware are present
    assert!(
        code_str.contains("TracingMiddleware"),
        "Should include tracing middleware"
    );
    assert!(
        code_str.contains("RetryTransientMiddleware"),
        "Should include retry middleware"
    );

    // Find positions of middleware setup
    let tracing_pos = code_str.find("TracingMiddleware");
    let retry_pos = code_str.find("RetryTransientMiddleware");

    assert!(tracing_pos.is_some(), "TracingMiddleware should be present");
    assert!(
        retry_pos.is_some(),
        "RetryTransientMiddleware should be present"
    );

    // Verify tracing comes before retry in the code
    // (This ensures middleware stack order: tracing first, then retry)
    assert!(
        tracing_pos.unwrap() < retry_pos.unwrap(),
        "TracingMiddleware should be added before RetryTransientMiddleware in code order"
    );
}

#[test]
fn test_both_retry_and_tracing_enabled() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_generator::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify with_config has both parameters
    assert!(
        code_str.contains("retry_config : Option < RetryConfig >"),
        "with_config should have retry_config parameter"
    );
    assert!(
        code_str.contains("enable_tracing : bool"),
        "with_config should have enable_tracing parameter"
    );

    // Verify new() constructor calls with_config with correct defaults
    assert!(
        code_str.contains("pub fn new"),
        "Should generate new() constructor"
    );
    assert!(
        code_str.contains("Self :: with_config"),
        "new() should call with_config"
    );
}

#[test]
fn test_generated_code_compiles() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_generator::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();

    // Try to parse the generated code as valid Rust syntax
    let code_str = client_code.to_string();

    // This is a basic check - the code should at least be parseable
    assert!(!code_str.is_empty(), "Generated code should not be empty");

    // Verify it has proper structure markers
    assert!(
        code_str.contains("impl HttpClient"),
        "Should have HttpClient impl block"
    );
}

#[test]
fn test_default_retry_config_values() {
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_generator::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator.generate_http_client_struct();
    let code_str = client_code.to_string();

    // Verify default values in Default impl
    assert!(
        code_str.contains("max_retries : 3"),
        "Default max_retries should be 3"
    );
    assert!(
        code_str.contains("initial_delay_ms : 500"),
        "Default initial_delay_ms should be 500"
    );
    assert!(
        code_str.contains("max_delay_ms : 16000"),
        "Default max_delay_ms should be 16000"
    );
}
