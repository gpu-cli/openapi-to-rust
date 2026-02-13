use openapi_generator::{CodeGenerator, GeneratorConfig};
use std::path::PathBuf;

fn main() {
    // Example 1: Client with both retry and tracing
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

    println!("=== GENERATED HTTP CLIENT (WITH RETRY + TRACING) ===\n");
    println!("{}", client_code);

    // Example 2: Minimal client without middleware
    println!("\n\n=== GENERATED HTTP CLIENT (MINIMAL - NO MIDDLEWARE) ===\n");
    let minimal_config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: None,
        tracing_enabled: false,
        ..Default::default()
    };

    let minimal_generator = CodeGenerator::new(minimal_config);
    let minimal_client = minimal_generator.generate_http_client_struct();
    println!("{}", minimal_client);
}
