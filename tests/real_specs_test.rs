use openapi_generator::GeneratorConfig;

/// Test to verify backward compatibility with the existing Rust API
/// This ensures we haven't broken existing usage patterns
#[test]
fn test_backward_compatibility_rust_api() {
    // Verify GeneratorConfig can still be used without TOML
    let config = GeneratorConfig {
        enable_async_client: true,
        enable_sse_client: true,
        ..Default::default()
    };

    // Should compile and work
    assert!(config.enable_async_client);
    assert!(config.enable_sse_client);

    // Verify default values work (both default to true in Phase 7)
    let default_config = GeneratorConfig::default();
    assert!(default_config.enable_async_client);
    assert!(default_config.enable_sse_client);
}
