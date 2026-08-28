use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec with inline object schemas in unions
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Inline Objects Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "name": {
                            "type": "string"
                        }
                    },
                    "required": ["id", "name"]
                },
                "EventUnion": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/User"},
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["guest"]
                                },
                                "sessionId": {
                                    "type": "string"
                                },
                                "timestamp": {
                                    "type": "string",
                                    "format": "date-time"
                                }
                            },
                            "required": ["type", "sessionId"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["system"]
                                },
                                "message": {
                                    "type": "string"
                                },
                                "level": {
                                    "type": "string",
                                    "enum": ["info", "warning", "error"]
                                }
                            },
                            "required": ["type", "message", "level"]
                        }
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    },
                    "description": "Union with inline object schemas"
                },
                "FlexibleData": {
                    "anyOf": [
                        {"$ref": "#/components/schemas/User"},
                        {
                            "type": "object",
                            "properties": {
                                "data": {
                                    "type": "string"
                                },
                                "format": {
                                    "type": "string",
                                    "enum": ["json", "xml", "text"]
                                }
                            },
                            "required": ["data"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "bytes": {
                                    "type": "string",
                                    "format": "byte"
                                },
                                "encoding": {
                                    "type": "string",
                                    "enum": ["base64", "hex"]
                                }
                            },
                            "required": ["bytes", "encoding"]
                        }
                    ],
                    "description": "Flexible union with inline objects (anyOf)"
                }
            }
        }
    });

    println!("Creating schema analyzer for inline objects test...");
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
                    ..
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
                openapi_to_rust::analysis::SchemaType::Union { variants, .. } => {
                    let variant_info: Vec<String> =
                        variants.iter().map(|v| v.target.to_string()).collect();
                    format!("Union(variants: [{}])", variant_info.join(", "))
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
    println!(
        "  Untagged enums: {:?}",
        analysis.patterns.untagged_enum_schemas
    );

    println!("\nDependency graph:");
    for (schema, deps) in &analysis.dependencies.edges {
        if !deps.is_empty() {
            println!("  {schema} depends on: {deps:?}");
        }
    }

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "inline_objects_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
