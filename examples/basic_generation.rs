use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple OpenAPI spec for testing
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "integer",
                            "description": "User ID"
                        },
                        "name": {
                            "type": "string",
                            "description": "User name"
                        },
                        "email": {
                            "type": "string",
                            "format": "email"
                        },
                        "status": {
                            "$ref": "#/components/schemas/UserStatus"
                        }
                    },
                    "required": ["id", "name"]
                },
                "UserStatus": {
                    "type": "string",
                    "enum": ["active", "inactive", "pending"],
                    "description": "User status"
                },
                "ApiResponse": {
                    "type": "object",
                    "properties": {
                        "success": {
                            "type": "boolean"
                        },
                        "message": {
                            "type": "string"
                        },
                        "data": {
                            "anyOf": [
                                {"$ref": "#/components/schemas/User"},
                                {"type": "null"}
                            ]
                        }
                    },
                    "required": ["success"]
                }
            }
        }
    });

    println!("Creating schema analyzer...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!("  - {}: {:?}", name, schema.schema_type);
    }

    println!("\nDetected patterns:");
    println!(
        "  Tagged enums: {:?}",
        analysis.patterns.tagged_enum_schemas
    );
    println!(
        "  Untagged enums: {:?}",
        analysis.patterns.untagged_enum_schemas
    );

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "test_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
