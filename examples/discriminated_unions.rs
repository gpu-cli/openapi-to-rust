use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec with discriminated unions (oneOf)
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Discriminated Union Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "ResponseCreatedEvent": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["response.created"]
                        },
                        "response": {
                            "$ref": "#/components/schemas/Response"
                        }
                    },
                    "required": ["type", "response"]
                },
                "ResponseTextDeltaEvent": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["response.text.delta"]
                        },
                        "delta": {
                            "type": "string"
                        },
                        "response_id": {
                            "type": "string"
                        }
                    },
                    "required": ["type", "delta", "response_id"]
                },
                "ResponseCompletedEvent": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["response.completed"]
                        },
                        "response": {
                            "$ref": "#/components/schemas/Response"
                        }
                    },
                    "required": ["type", "response"]
                },
                "ResponseStreamEvent": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/ResponseCreatedEvent"},
                        {"$ref": "#/components/schemas/ResponseTextDeltaEvent"},
                        {"$ref": "#/components/schemas/ResponseCompletedEvent"}
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    },
                    "description": "Events that can be received when streaming a response"
                },
                "Response": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "object": {
                            "type": "string",
                            "enum": ["response"]
                        },
                        "status": {
                            "type": "string",
                            "enum": ["in_progress", "completed", "failed"]
                        }
                    },
                    "required": ["id", "object", "status"]
                },
                "Animal": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/Dog"},
                        {"$ref": "#/components/schemas/Cat"}
                    ],
                    "discriminator": {
                        "propertyName": "species"
                    }
                },
                "Dog": {
                    "type": "object",
                    "properties": {
                        "species": {
                            "type": "string",
                            "enum": ["dog"]
                        },
                        "breed": {
                            "type": "string"
                        },
                        "bark_volume": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 10
                        }
                    },
                    "required": ["species", "breed"]
                },
                "Cat": {
                    "type": "object",
                    "properties": {
                        "species": {
                            "type": "string",
                            "enum": ["cat"]
                        },
                        "breed": {
                            "type": "string"
                        },
                        "purr_frequency": {
                            "type": "number"
                        }
                    },
                    "required": ["species", "breed"]
                }
            }
        }
    });

    println!("Creating schema analyzer for discriminated union test...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!(
            "  - {}: {:?}",
            name,
            match &schema.schema_type {
                openapi_to_rust::analysis::SchemaType::DiscriminatedUnion {
                    discriminator_field,
                    variants,
                } => {
                    format!(
                        "DiscriminatedUnion(discriminator: {}, variants: {})",
                        discriminator_field,
                        variants.len()
                    )
                }
                other => format!("{other:?}"),
            }
        );
    }

    println!("\nDetected patterns:");
    println!(
        "  Tagged enums: {:?}",
        analysis.patterns.tagged_enum_schemas
    );

    println!("\nDependency graph:");
    for (schema, deps) in &analysis.dependencies.edges {
        if !deps.is_empty() {
            println!("  {schema} depends on: {deps:?}");
        }
    }

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "discriminated_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
