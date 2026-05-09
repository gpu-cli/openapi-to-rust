use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec with number format testing
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Number Formats Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "NumberFormats": {
                    "type": "object",
                    "properties": {
                        "int32_field": {
                            "type": "integer",
                            "format": "int32",
                            "description": "32-bit integer"
                        },
                        "int64_field": {
                            "type": "integer",
                            "format": "int64",
                            "description": "64-bit integer"
                        },
                        "float_field": {
                            "type": "number",
                            "format": "float",
                            "description": "32-bit float"
                        },
                        "double_field": {
                            "type": "number",
                            "format": "double",
                            "description": "64-bit double"
                        },
                        "default_integer": {
                            "type": "integer",
                            "description": "Integer without format (should be i64)"
                        },
                        "default_number": {
                            "type": "number",
                            "description": "Number without format (should be f64)"
                        }
                    },
                    "required": ["int32_field", "float_field"]
                },
                "ArrayFormats": {
                    "type": "object",
                    "properties": {
                        "int32_array": {
                            "type": "array",
                            "items": {
                                "type": "integer",
                                "format": "int32"
                            }
                        },
                        "float_array": {
                            "type": "array",
                            "items": {
                                "type": "number",
                                "format": "float"
                            }
                        }
                    }
                }
            }
        }
    });

    println!("Creating schema analyzer for number formats test...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!(
            "  - {}: {:?}",
            name,
            match &schema.schema_type {
                openapi_to_rust::analysis::SchemaType::Object { properties, .. } => {
                    let mut prop_types = Vec::new();
                    for (prop_name, prop_info) in properties {
                        if let openapi_to_rust::analysis::SchemaType::Primitive {
                            rust_type, ..
                        } = &prop_info.schema_type
                        {
                            prop_types.push(format!("{prop_name}: {rust_type}"));
                        } else {
                            prop_types.push(format!("{}: {:?}", prop_name, prop_info.schema_type));
                        }
                    }
                    format!("Object({})", prop_types.join(", "))
                }
                other => format!("{other:?}"),
            }
        );
    }

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "number_formats_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
