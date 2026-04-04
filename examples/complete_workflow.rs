// Complete workflow example showing the entire code generation and usage process
//
// This example demonstrates:
// 1. Loading an OpenAPI spec
// 2. Configuring the generator with TOML (or Rust API)
// 3. Generating code
// 4. Using the generated HTTP client
// 5. Error handling and retry logic

use openapi_to_rust::{
    CodeGenerator, GeneratorConfig, SchemaAnalyzer, config::ConfigFile, http_config::RetryConfig,
};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== OpenAPI Generator - Complete Workflow Example ===\n");

    // Step 1: Create a simple OpenAPI spec for demonstration
    println!("Step 1: Creating sample OpenAPI specification...");
    let openapi_spec = serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Example API",
            "version": "1.0.0"
        },
        "paths": {
            "/items": {
                "get": {
                    "operationId": "listItems",
                    "summary": "List all items",
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/ItemList"
                                    }
                                }
                            }
                        }
                    }
                },
                "post": {
                    "operationId": "createItem",
                    "summary": "Create a new item",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/CreateItemRequest"
                                }
                            }
                        }
                    },
                    "responses": {
                        "201": {
                            "description": "Created",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/Item"
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/items/{id}": {
                "get": {
                    "operationId": "getItem",
                    "summary": "Get item by ID",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {
                                "type": "string"
                            }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/Item"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Item": {
                    "type": "object",
                    "required": ["id", "name"],
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Unique identifier"
                        },
                        "name": {
                            "type": "string",
                            "description": "Item name"
                        },
                        "description": {
                            "type": "string",
                            "description": "Item description"
                        },
                        "created_at": {
                            "type": "integer",
                            "description": "Creation timestamp"
                        }
                    }
                },
                "CreateItemRequest": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "description": {
                            "type": "string"
                        }
                    }
                },
                "ItemList": {
                    "type": "object",
                    "required": ["items"],
                    "properties": {
                        "items": {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/Item"
                            }
                        },
                        "total": {
                            "type": "integer"
                        }
                    }
                }
            }
        }
    });

    // Create temporary directory for output
    let temp_dir = TempDir::new()?;
    let spec_path = temp_dir.path().join("openapi.json");
    let output_dir = temp_dir.path().join("generated");

    std::fs::write(&spec_path, serde_json::to_string_pretty(&openapi_spec)?)?;
    println!("  Created spec at: {}", spec_path.display());

    // Step 2: Option A - Using TOML Configuration
    println!("\nStep 2A: Demonstrating TOML configuration approach...");
    demonstrate_toml_config(&spec_path, &output_dir)?;

    // Step 2: Option B - Using Rust API
    println!("\nStep 2B: Demonstrating Rust API approach...");
    demonstrate_rust_api(&spec_path, &output_dir)?;

    // Step 3: Show the generated code structure
    println!("\nStep 3: Generated code structure:");
    show_generated_structure(&output_dir)?;

    // Step 4: Demonstrate usage patterns
    println!("\nStep 4: Usage patterns (pseudocode):");
    show_usage_patterns();

    // Step 5: Error handling examples
    println!("\nStep 5: Error handling patterns:");
    show_error_handling();

    println!("\n=== Workflow Complete ===");
    println!("Generated code is in: {}", output_dir.display());
    println!("\nNext steps:");
    println!("1. Review generated files in the output directory");
    println!("2. Add generated module to your project");
    println!("3. Configure HttpClient with your API credentials");
    println!("4. Make API calls using the generated methods");

    Ok(())
}

fn demonstrate_toml_config(
    spec_path: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Creating TOML configuration...");

    let toml_config = format!(
        r#"
[generator]
spec_path = "{}"
output_dir = "{}"
module_name = "api"

[features]
enable_async_client = true
enable_specta = false

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 30

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
"#,
        spec_path.display(),
        output_dir.display()
    );

    let config_path = output_dir.parent().unwrap().join("config.toml");
    std::fs::write(&config_path, toml_config)?;
    println!("  Created config at: {}", config_path.display());

    // Load and use the config
    let config_file = ConfigFile::load(&config_path)?;
    let generator_config = config_file.into_generator_config();

    println!("  Loaded configuration:");
    println!(
        "    - Base URL: {:?}",
        generator_config
            .http_client_config
            .as_ref()
            .and_then(|c| c.base_url.as_ref())
    );
    println!(
        "    - Retry enabled: {}",
        generator_config.retry_config.is_some()
    );
    println!(
        "    - Tracing enabled: {}",
        generator_config.tracing_enabled
    );

    Ok(())
}

fn demonstrate_rust_api(
    spec_path: &PathBuf,
    output_dir: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("  Configuring with Rust API...");

    // Read and parse the spec
    let spec_content = std::fs::read_to_string(spec_path)?;
    let spec_value: serde_json::Value = serde_json::from_str(&spec_content)?;

    // Analyze the schema
    println!("  Analyzing OpenAPI schema...");
    let mut analyzer = SchemaAnalyzer::new(spec_value)?;
    let mut analysis = analyzer.analyze()?;

    println!("    - Found {} schemas", analysis.schemas.len());
    println!("    - Found {} operations", analysis.operations.len());

    // Configure the generator
    use openapi_to_rust::http_config::{AuthConfig, HttpClientConfig};

    let config = GeneratorConfig {
        spec_path: spec_path.clone(),
        output_dir: output_dir.clone(),
        module_name: "api".to_string(),
        enable_sse_client: false,
        enable_async_client: true,
        enable_specta: false,
        http_client_config: Some(HttpClientConfig {
            base_url: Some("https://api.example.com".to_string()),
            timeout_seconds: Some(30),
            default_headers: {
                let mut headers = std::collections::HashMap::new();
                headers.insert("content-type".to_string(), "application/json".to_string());
                headers
            },
        }),
        retry_config: Some(RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: true,
        auth_config: Some(AuthConfig::Bearer {
            header_name: "Authorization".to_string(),
        }),
        type_mappings: std::collections::BTreeMap::new(),
        streaming_config: None,
        nullable_field_overrides: std::collections::BTreeMap::new(),
        schema_extensions: vec![],
        enable_registry: false,
        registry_only: false,
    };

    // Generate code
    println!("  Generating code...");
    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis)?;

    // Write files
    std::fs::create_dir_all(output_dir)?;
    generator.write_files(&result)?;

    println!("  Generated {} files:", result.files.len());
    for file in &result.files {
        println!("    - {}", file.path.display());
    }

    Ok(())
}

fn show_generated_structure(output_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if !output_dir.exists() {
        println!("  Output directory not found (example only)");
        return Ok(());
    }

    println!("  Generated files:");
    for entry in std::fs::read_dir(output_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let size = entry.metadata()?.len();
            println!(
                "    - {} ({} bytes)",
                path.file_name().unwrap().to_string_lossy(),
                size
            );
        }
    }

    Ok(())
}

fn show_usage_patterns() {
    println!(
        r#"
  Basic Usage:
  ------------
  use crate::generated::{{client::HttpClient, types::*}};

  #[tokio::main]
  async fn main() -> Result<(), Box<dyn std::error::Error>> {{
      // Create client
      let client = HttpClient::new()
          .with_base_url("https://api.example.com")
          .with_api_key("your-api-key")
          .with_header("X-Custom-Header".to_string(), "value".to_string());

      // List items (GET /items)
      let items = client.list_items().await?;
      println!("Found {{}} items", items.items.len());

      // Create item (POST /items)
      let new_item = CreateItemRequest {{
          name: "New Item".to_string(),
          description: Some("Description".to_string()),
      }};
      let created = client.create_item(new_item).await?;
      println!("Created item: {{}}", created.id);

      // Get item by ID (GET /items/{{id}})
      let item = client.get_item("item-123").await?;
      println!("Item: {{:?}}", item);

      Ok(())
  }}

  Advanced Configuration:
  -----------------------
  use crate::generated::client::{{HttpClient, RetryConfig}};

  let client = HttpClient::with_config(
      Some(RetryConfig {{
          max_retries: 5,
          initial_delay_ms: 1000,
          max_delay_ms: 30000,
      }}),
      true, // enable tracing
  )
  .with_base_url("https://api.example.com")
  .with_api_key("your-api-key");
"#
    );
}

fn show_error_handling() {
    println!(
        r#"
  Error Handling:
  ---------------
  use crate::generated::http_error::HttpError;

  match client.create_item(request).await {{
      Ok(item) => {{
          println!("Success: {{:?}}", item);
      }}
      Err(HttpError::Network(e)) => {{
          eprintln!("Network error: {{}}", e);
          // Will be retried automatically if retry is configured
      }}
      Err(HttpError::Http {{ status, message, .. }}) => {{
          match status {{
              400 => eprintln!("Bad request: {{}}", message),
              401 => eprintln!("Unauthorized - check API key"),
              404 => eprintln!("Not found: {{}}", message),
              429 => eprintln!("Rate limited - will retry"),
              500..=599 => eprintln!("Server error: {{}}", message),
              _ => eprintln!("HTTP error {{}}: {{}}", status, message),
          }}
      }}
      Err(HttpError::Timeout) => {{
          eprintln!("Request timeout - will retry");
      }}
      Err(HttpError::Auth(msg)) => {{
          eprintln!("Authentication error: {{}}", msg);
      }}
      Err(e) => {{
          eprintln!("Other error: {{}}", e);
      }}
  }}

  Retry Detection:
  ----------------
  match client.request().await {{
      Err(e) if e.is_retryable() => {{
          // Error will be automatically retried by middleware
          println!("Retryable error detected");
      }}
      Err(e) if e.is_client_error() => {{
          // 4xx errors - fix request and retry
          println!("Client error: fix request");
      }}
      Err(e) if e.is_server_error() => {{
          // 5xx errors - may be retried automatically
          println!("Server error");
      }}
      _ => {{}}
  }}
"#
    );
}
