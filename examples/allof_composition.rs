use openapi_generator::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec with allOf composition
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Composition Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "BaseEntity": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string",
                            "description": "Unique identifier"
                        },
                        "created_at": {
                            "type": "string",
                            "format": "date-time"
                        }
                    },
                    "required": ["id"]
                },
                "PersonalInfo": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "email": {
                            "type": "string",
                            "format": "email"
                        }
                    },
                    "required": ["name"]
                },
                "User": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseEntity"},
                        {"$ref": "#/components/schemas/PersonalInfo"},
                        {
                            "type": "object",
                            "properties": {
                                "role": {
                                    "type": "string",
                                    "enum": ["user", "admin", "moderator"]
                                },
                                "is_active": {
                                    "type": "boolean"
                                }
                            },
                            "required": ["role"]
                        }
                    ],
                    "description": "A user combining base entity, personal info, and user-specific fields"
                },
                "Product": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseEntity"},
                        {
                            "type": "object",
                            "properties": {
                                "name": {
                                    "type": "string"
                                },
                                "price": {
                                    "type": "number",
                                    "minimum": 0
                                },
                                "category": {
                                    "$ref": "#/components/schemas/Category"
                                }
                            },
                            "required": ["name", "price"]
                        }
                    ]
                },
                "Category": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "description": {
                            "type": "string"
                        }
                    },
                    "required": ["name"]
                }
            }
        }
    });

    println!("Creating schema analyzer for allOf composition test...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!("  - {}: {:?}", name, schema.schema_type);
    }

    println!("\nDependency graph:");
    for (schema, deps) in &analysis.dependencies.edges {
        if !deps.is_empty() {
            println!("  {schema} depends on: {deps:?}");
        }
    }

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "composition_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
