//! Example: Generate a complete HTTP client from an OpenAPI specification
//!
//! This example demonstrates how to:
//! 1. Load an OpenAPI spec
//! 2. Configure the generator with retry and tracing
//! 3. Generate a complete HTTP client
//! 4. Save the generated code to files
//!
//! Run this example:
//! ```bash
//! cargo run --example generate_full_client
//! ```

use openapi_generator::{CodeGenerator, GeneratorConfig, analysis::SchemaAnalyzer};
use serde_json::json;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 OpenAPI HTTP Client Generator Example\n");

    // 1. Create a sample OpenAPI specification
    println!("📝 Creating sample OpenAPI specification...");
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Todo API",
            "version": "1.0.0",
            "description": "A simple Todo API for demonstration"
        },
        "servers": [
            {
                "url": "https://api.example.com/v1"
            }
        ],
        "paths": {
            "/todos": {
                "get": {
                    "operationId": "listTodos",
                    "summary": "List all todos",
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": {
                                            "$ref": "#/components/schemas/Todo"
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "post": {
                    "operationId": "createTodo",
                    "summary": "Create a new todo",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/CreateTodoRequest"
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
                                        "$ref": "#/components/schemas/Todo"
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/todos/{id}": {
                "get": {
                    "operationId": "getTodo",
                    "summary": "Get a specific todo",
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
                                        "$ref": "#/components/schemas/Todo"
                                    }
                                }
                            }
                        }
                    }
                },
                "put": {
                    "operationId": "updateTodo",
                    "summary": "Update a todo",
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
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/UpdateTodoRequest"
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/Todo"
                                    }
                                }
                            }
                        }
                    }
                },
                "delete": {
                    "operationId": "deleteTodo",
                    "summary": "Delete a todo",
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
                        "204": {
                            "description": "No Content"
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "Todo": {
                    "type": "object",
                    "required": ["id", "title", "completed"],
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Unique identifier for the todo"
                        },
                        "title": {
                            "type": "string",
                            "description": "The todo item title"
                        },
                        "completed": {
                            "type": "boolean",
                            "description": "Whether the todo is completed",
                            "default": false
                        },
                        "description": {
                            "type": "string",
                            "description": "Optional description"
                        }
                    }
                },
                "CreateTodoRequest": {
                    "type": "object",
                    "required": ["title"],
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "The todo item title"
                        },
                        "description": {
                            "type": "string",
                            "description": "Optional description"
                        }
                    }
                },
                "UpdateTodoRequest": {
                    "type": "object",
                    "required": ["title", "completed"],
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "The todo item title"
                        },
                        "completed": {
                            "type": "boolean",
                            "description": "Whether the todo is completed"
                        },
                        "description": {
                            "type": "string",
                            "description": "Optional description"
                        }
                    }
                }
            }
        }
    });

    // 2. Analyze the OpenAPI specification
    println!("🔍 Analyzing OpenAPI specification...");
    let mut analyzer = SchemaAnalyzer::new(spec)?;
    let mut analysis = analyzer.analyze()?;
    println!("   ✅ Found {} schemas", analysis.schemas.len());
    println!("   ✅ Found {} operations", analysis.operations.len());

    // 3. Configure the generator with retry and tracing
    println!("\n⚙️  Configuring generator...");
    let config = GeneratorConfig {
        spec_path: PathBuf::from("todo-api.json"),
        output_dir: PathBuf::from("examples/generated"),
        module_name: "todo_api".to_string(),
        enable_async_client: true,
        enable_sse_client: false,
        enable_specta: false,
        retry_config: Some(openapi_generator::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: true,
        ..Default::default()
    };

    println!("   ✅ HTTP client enabled with:");
    println!("      - Retry: max 3 retries with exponential backoff");
    println!("      - Tracing: enabled for request/response logging");

    // 4. Generate all files
    println!("\n🏗️  Generating code...");
    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis)?;

    println!("   ✅ Generated {} files:", result.files.len() + 1);
    for file in &result.files {
        let path = file.path.display();
        let lines = file.content.lines().count();
        println!("      - {} ({} lines)", path, lines);
    }
    println!(
        "      - {} ({} lines)",
        result.mod_file.path.display(),
        result.mod_file.content.lines().count()
    );

    // 5. Show a preview of the generated client
    println!("\n📄 Client preview:");
    if let Some(client_file) = result
        .files
        .iter()
        .find(|f| f.path.to_str() == Some("client.rs"))
    {
        // Show first 30 lines of the generated client
        let preview_lines: Vec<&str> = client_file.content.lines().take(30).collect();
        for line in preview_lines {
            println!("   {}", line);
        }
        println!(
            "   ... ({} total lines)",
            client_file.content.lines().count()
        );
    }

    // 6. Save to disk
    println!("\n💾 Saving files to disk...");
    generator.write_files(&result)?;
    println!("   ✅ Files written to: examples/generated/");

    // 7. Show example usage
    println!("\n📚 Example usage of the generated client:");
    println!(
        r#"
use todo_api::{{HttpClient, CreateTodoRequest}};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {{
    // Create a client with retry and tracing
    let client = HttpClient::new()
        .with_base_url("https://api.example.com/v1")
        .with_api_key("your-api-key");

    // List all todos
    let todos = client.list_todos().await?;
    println!("Found {{}} todos", todos.len());

    // Create a new todo
    let new_todo = CreateTodoRequest {{
        title: "Buy groceries".to_string(),
        description: Some("Milk, eggs, bread".to_string()),
    }};
    let created = client.create_todo(new_todo).await?;
    println!("Created todo: {{}}", created.id);

    // Get a specific todo
    let todo = client.get_todo(&created.id).await?;
    println!("Todo: {{}} - completed: {{}}", todo.title, todo.completed);

    // Update the todo
    let update = UpdateTodoRequest {{
        title: todo.title.clone(),
        completed: true,
        description: todo.description,
    }};
    let updated = client.update_todo(&created.id, update).await?;
    println!("Updated todo completed status: {{}}", updated.completed);

    // Delete the todo
    client.delete_todo(&created.id).await?;
    println!("Todo deleted");

    Ok(())
}}
"#
    );

    println!("\n✨ Generation complete!");
    println!("\n💡 Next steps:");
    println!("   1. Review the generated code in examples/generated/");
    println!("   2. Copy the generated files to your project");
    println!("   3. Add the required dependencies to your Cargo.toml:");
    println!("      - reqwest = {{ version = \"0.12\", features = [\"json\"] }}");
    println!("      - reqwest-middleware = \"0.3\"");
    println!("      - reqwest-retry = \"0.6\"");
    println!("      - reqwest-tracing = \"0.5\"");
    println!("      - serde = {{ version = \"1.0\", features = [\"derive\"] }}");
    println!("      - serde_json = \"1.0\"");
    println!("      - thiserror = \"1.0\"");
    println!("      - tokio = {{ version = \"1\", features = [\"full\"] }}");

    Ok(())
}
