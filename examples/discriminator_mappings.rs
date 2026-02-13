use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec with explicit discriminator mappings
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Discriminator Mappings Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Cat": {
                    "type": "object",
                    "properties": {
                        "petType": {
                            "type": "string",
                            "enum": ["cat"]
                        },
                        "name": {
                            "type": "string"
                        },
                        "huntingSkill": {
                            "type": "string",
                            "enum": ["clueless", "lazy", "adventurous", "aggressive"]
                        }
                    },
                    "required": ["petType", "name"]
                },
                "Dog": {
                    "type": "object",
                    "properties": {
                        "petType": {
                            "type": "string",
                            "enum": ["dog"]
                        },
                        "name": {
                            "type": "string"
                        },
                        "packSize": {
                            "type": "integer",
                            "format": "int32",
                            "minimum": 0
                        }
                    },
                    "required": ["petType", "name"]
                },
                "Bird": {
                    "type": "object",
                    "properties": {
                        "petType": {
                            "type": "string",
                            "enum": ["bird"]
                        },
                        "name": {
                            "type": "string"
                        },
                        "wingspan": {
                            "type": "number",
                            "format": "float",
                            "minimum": 0
                        }
                    },
                    "required": ["petType", "name"]
                },
                "Pet": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/Cat"},
                        {"$ref": "#/components/schemas/Dog"},
                        {"$ref": "#/components/schemas/Bird"}
                    ],
                    "discriminator": {
                        "propertyName": "petType",
                        "mapping": {
                            "cat": "#/components/schemas/Cat",
                            "dog": "#/components/schemas/Dog",
                            "bird": "#/components/schemas/Bird"
                        }
                    },
                    "description": "A pet that can be a cat, dog, or bird with explicit mappings"
                },
                "VehicleEngine": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["engine"]
                        },
                        "horsepower": {
                            "type": "integer"
                        }
                    },
                    "required": ["type"]
                },
                "VehicleElectric": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["electric"]
                        },
                        "batteryCapacity": {
                            "type": "number"
                        }
                    },
                    "required": ["type"]
                },
                "Vehicle": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/VehicleEngine"},
                        {"$ref": "#/components/schemas/VehicleElectric"}
                    ],
                    "discriminator": {
                        "propertyName": "type",
                        "mapping": {
                            "gas": "#/components/schemas/VehicleEngine",
                            "electric": "#/components/schemas/VehicleElectric"
                        }
                    },
                    "description": "Vehicle with custom discriminator values that don't match enum values"
                }
            }
        }
    });

    println!("Creating schema analyzer for discriminator mappings test...");
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
                    let variant_info: Vec<String> = variants
                        .iter()
                        .map(|v| {
                            format!(
                                "{}='{}' -> {}",
                                v.rust_name, v.discriminator_value, v.type_name
                            )
                        })
                        .collect();
                    format!(
                        "DiscriminatedUnion(discriminator: {}, variants: [{}])",
                        discriminator_field,
                        variant_info.join(", ")
                    )
                }
                openapi_to_rust::analysis::SchemaType::Object { properties, .. } => {
                    format!("Object({} properties)", properties.len())
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
    println!("  Type mappings: {:?}", analysis.patterns.type_mappings);

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "discriminator_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
