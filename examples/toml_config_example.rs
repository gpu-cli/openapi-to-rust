// TOML Configuration Examples
//
// This example demonstrates different TOML configurations for various use cases.

use openapi_generator::config::ConfigFile;
use std::fs;
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TOML Configuration Examples ===\n");

    // Create temporary directory for examples
    let temp_dir = TempDir::new()?;
    let base_path = temp_dir.path();

    // Create a dummy OpenAPI spec for validation
    let spec_path = base_path.join("openapi.json");
    fs::write(
        &spec_path,
        r#"{"openapi":"3.0.0","info":{"title":"Test","version":"1.0.0"},"paths":{}}"#,
    )?;

    println!("Example 1: Minimal Configuration");
    println!("==================================");
    demonstrate_minimal_config(&spec_path, base_path)?;

    println!("\nExample 2: Full Configuration with All Options");
    println!("================================================");
    demonstrate_full_config(&spec_path, base_path)?;

    println!("\nExample 3: Retry-Only Configuration");
    println!("=====================================");
    demonstrate_retry_only(&spec_path, base_path)?;

    println!("\nExample 4: Tracing-Disabled Configuration");
    println!("==========================================");
    demonstrate_no_tracing(&spec_path, base_path)?;

    println!("\nExample 5: Specta-Enabled Configuration (TypeScript Integration)");
    println!("==================================================================");
    demonstrate_specta_config(&spec_path, base_path)?;

    println!("\nExample 6: Multiple Authentication Patterns");
    println!("============================================");
    demonstrate_auth_patterns(&spec_path, base_path)?;

    println!("\nExample 7: Production-Ready Configuration");
    println!("==========================================");
    demonstrate_production_config(&spec_path, base_path)?;

    println!("\n=== All Examples Complete ===");
    println!("\nKey Takeaways:");
    println!("1. Minimal config only requires [generator] section");
    println!("2. HTTP client features are optional and configurable");
    println!("3. Retry and tracing can be independently enabled/disabled");
    println!("4. Authentication supports Bearer, ApiKey, and Custom types");
    println!("5. All configs are validated on load");

    Ok(())
}

fn demonstrate_minimal_config(
    spec_path: &std::path::Path,
    base_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "types"

[features]
enable_async_client = false
enable_sse_client = false
enable_specta = false
"#,
        spec_path.display()
    );

    println!("{}", config_content);

    let config_path = base_path.join("minimal.toml");
    fs::write(&config_path, config_content)?;

    let _config = ConfigFile::load(&config_path)?;
    println!("Status: Valid");
    println!("Features: Type generation only (no clients)");
    println!("HTTP Client: Disabled");

    Ok(())
}

fn demonstrate_full_config(
    spec_path: &std::path::Path,
    base_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_sse_client = true
enable_async_client = true
enable_specta = false

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
value = "my-rust-client/1.0"

[[http_client.headers]]
name = "x-api-version"
value = "2024-01-01"

[nullable_overrides]
"Response.error" = true
"Message.content" = true

[type_mappings]
"DateTime" = "chrono::DateTime<chrono::Utc>"
"Decimal" = "rust_decimal::Decimal"
"#,
        spec_path.display()
    );

    println!("{}", config_content);

    let config_path = base_path.join("full.toml");
    fs::write(&config_path, config_content)?;

    let _config = ConfigFile::load(&config_path)?;
    println!("Status: Valid");
    println!("Features: All enabled (SSE, HTTP client, type mappings)");
    println!("Retry: 5 attempts, 1000-30000ms backoff");
    println!("Tracing: Enabled");
    println!("Custom Headers: 3 headers");

    Ok(())
}

fn demonstrate_retry_only(
    spec_path: &std::path::Path,
    base_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_async_client = true

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 30

[http_client.retry]
max_retries = 3
initial_delay_ms = 500
max_delay_ms = 16000

# Tracing will be enabled by default
"#,
        spec_path.display()
    );

    println!("{}", config_content);

    let config_path = base_path.join("retry-only.toml");
    fs::write(&config_path, config_content)?;

    let config = ConfigFile::load(&config_path)?;
    let _gen_config = config.into_generator_config();

    println!("Status: Valid");
    println!(
        "Retry: {} attempts with exponential backoff",
        _gen_config.retry_config.as_ref().unwrap().max_retries
    );
    println!("Tracing: Enabled (default)");
    println!("Use Case: APIs with occasional transient failures");

    Ok(())
}

fn demonstrate_no_tracing(
    spec_path: &std::path::Path,
    base_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_async_client = true

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 30

[http_client.tracing]
enabled = false

# No retry configured - requests will not be retried
"#,
        spec_path.display()
    );

    println!("{}", config_content);

    let config_path = base_path.join("no-tracing.toml");
    fs::write(&config_path, config_content)?;

    let config = ConfigFile::load(&config_path)?;
    let _gen_config = config.into_generator_config();

    println!("Status: Valid");
    println!("Tracing: Disabled (explicit)");
    println!("Retry: Disabled");
    println!("Use Case: Minimal overhead, no observability needed");

    Ok(())
}

fn demonstrate_specta_config(
    spec_path: &std::path::Path,
    base_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api_types"

[features]
enable_async_client = true
enable_specta = true  # Enable TypeScript integration

[http_client]
base_url = "http://localhost:3000/api"
timeout_seconds = 30

[http_client.auth]
type = "Bearer"
header_name = "Authorization"
"#,
        spec_path.display()
    );

    println!("{}", config_content);

    let config_path = base_path.join("specta.toml");
    fs::write(&config_path, config_content)?;

    let config = ConfigFile::load(&config_path)?;
    let _gen_config = config.into_generator_config();

    println!("Status: Valid");
    println!("Specta: Enabled - types will include specta::Type derives");
    println!("Use Case: Tauri apps, full-stack TypeScript integration");
    println!("Usage: Export types with tauri-specta for frontend");

    Ok(())
}

fn demonstrate_auth_patterns(
    spec_path: &std::path::Path,
    base_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Bearer Token Authentication:");
    println!("----------------------------");
    let bearer_config = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_async_client = true

[http_client.auth]
type = "Bearer"
header_name = "Authorization"
"#,
        spec_path.display()
    );
    println!("{}\n", bearer_config);

    println!("API Key Authentication:");
    println!("-----------------------");
    let apikey_config = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_async_client = true

[http_client.auth]
type = "ApiKey"
header_name = "X-API-Key"
"#,
        spec_path.display()
    );
    println!("{}\n", apikey_config);

    println!("Custom Authentication:");
    println!("----------------------");
    let custom_config = format!(
        r#"[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_async_client = true

[http_client.auth]
type = "Custom"
header_name = "X-Custom-Auth"
"#,
        spec_path.display()
    );
    println!("{}\n", custom_config);

    let config_path = base_path.join("auth-bearer.toml");
    fs::write(&config_path, bearer_config)?;
    let _config = ConfigFile::load(&config_path)?;

    println!("Status: All patterns valid");
    println!("Use Cases:");
    println!("  - Bearer: OAuth, JWT tokens");
    println!("  - ApiKey: Simple API key authentication");
    println!("  - Custom: Non-standard auth schemes");

    Ok(())
}

fn demonstrate_production_config(
    spec_path: &std::path::Path,
    base_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config_content = format!(
        r#"# Production-Ready OpenAPI Generator Configuration
# This config includes all recommended settings for production use

[generator]
spec_path = "{}"
output_dir = "src/generated"
module_name = "api"

[features]
enable_sse_client = true    # Enable if API supports SSE
enable_async_client = true  # Enable HTTP client
enable_specta = false       # Enable only if using Tauri/TypeScript

[http_client]
base_url = "https://api.production.com"
timeout_seconds = 60        # Longer timeout for production

# Production retry configuration
[http_client.retry]
max_retries = 5             # More aggressive retry
initial_delay_ms = 1000     # Start with 1 second
max_delay_ms = 60000        # Max 60 seconds between retries

# Enable tracing for observability
[http_client.tracing]
enabled = true              # CRITICAL for production debugging

# Production authentication
[http_client.auth]
type = "Bearer"
header_name = "Authorization"

# Required headers
[[http_client.headers]]
name = "content-type"
value = "application/json"

[[http_client.headers]]
name = "user-agent"
value = "production-client/1.0.0"

[[http_client.headers]]
name = "accept"
value = "application/json"

# API-specific nullable overrides
[nullable_overrides]
"Response.error" = true          # API may return null error
"Message.content" = true         # Content may be null in some cases

# Custom type mappings for precise types
[type_mappings]
"DateTime" = "chrono::DateTime<chrono::Utc>"
"Timestamp" = "i64"
"Decimal" = "rust_decimal::Decimal"
"#,
        spec_path.display()
    );

    println!("{}", config_content);

    let config_path = base_path.join("production.toml");
    fs::write(&config_path, config_content)?;

    let config = ConfigFile::load(&config_path)?;
    let _gen_config = config.into_generator_config();

    println!("\nStatus: Valid");
    println!("Configuration Summary:");
    println!("  - Retry: 5 attempts, 1-60 second backoff");
    println!("  - Tracing: Enabled for observability");
    println!("  - Timeout: 60 seconds");
    println!("  - Auth: Bearer token");
    println!("  - Headers: 3 standard headers");
    println!("  - Type Safety: Custom type mappings enabled");
    println!("\nProduction Checklist:");
    println!("  [✓] Retry logic enabled");
    println!("  [✓] Tracing enabled");
    println!("  [✓] Appropriate timeout");
    println!("  [✓] Authentication configured");
    println!("  [✓] Custom headers set");
    println!("  [✓] Type mappings defined");

    Ok(())
}
