use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec that mimics the OpenAI Item discriminated union issue
    // where both InputMessage and OutputMessage have type: "message"
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Nested Discriminators Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "InputMessage": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["message"],
                            "description": "The type of the message input. Always set to `message`."
                        },
                        "role": {
                            "type": "string",
                            "enum": ["user", "system", "developer"]
                        },
                        "content": {
                            "type": "string"
                        }
                    },
                    "required": ["type", "role", "content"]
                },
                "OutputMessage": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["message"],
                            "description": "The type of the output message. Always `message`."
                        },
                        "role": {
                            "type": "string",
                            "enum": ["assistant"]
                        },
                        "content": {
                            "type": "array",
                            "items": {
                                "type": "string"
                            }
                        },
                        "id": {
                            "type": "string"
                        }
                    },
                    "required": ["type", "role", "content", "id"]
                },
                "FileSearchToolCall": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "enum": ["file_search_call"]
                        },
                        "id": {
                            "type": "string"
                        }
                    },
                    "required": ["type", "id"]
                },
                "Item": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/InputMessage"},
                        {"$ref": "#/components/schemas/OutputMessage"},
                        {"$ref": "#/components/schemas/FileSearchToolCall"}
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    },
                    "description": "Content item that demonstrates the discriminator collision issue"
                }
            }
        }
    });

    println!("Creating schema analyzer for nested discriminators test...");
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
        module_name: "nested_discriminator_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    // Find the Item enum in the generated code to verify discriminator values
    if let Some(item_start) = generated_code.find("pub enum Item") {
        if let Some(item_end) = generated_code[item_start..].find('}') {
            let item_enum = &generated_code[item_start..item_start + item_end + 1];
            println!("\n=== Item enum with discriminator values ===");
            println!("{item_enum}");
        }
    }

    Ok(())
}
