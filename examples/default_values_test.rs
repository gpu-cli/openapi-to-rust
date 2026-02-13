use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test default value patterns from OpenAI and other APIs
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Default Values Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Direction": {
                    "type": "string",
                    "enum": ["left", "right", "wheel", "back", "forward"],
                    "default": "left",
                    "description": "Multi-value enum with explicit default"
                },
                "Constant": {
                    "type": "string",
                    "enum": ["approximate"],
                    "default": "approximate",
                    "description": "Single-value enum with default"
                },
                "Settings": {
                    "type": "object",
                    "properties": {
                        "auto_save": {
                            "type": "boolean",
                            "default": true,
                            "description": "Auto-save enabled by default"
                        },
                        "max_retries": {
                            "type": "integer",
                            "default": 3,
                            "description": "Default retry count"
                        },
                        "timeout": {
                            "type": "number",
                            "default": 30.0,
                            "description": "Timeout in seconds"
                        },
                        "mode": {
                            "$ref": "#/components/schemas/Direction",
                            "description": "Default direction mode"
                        },
                        "description": {
                            "type": "string",
                            "default": "No description provided",
                            "description": "Default description text"
                        },
                        "tags": {
                            "type": "array",
                            "items": {"type": "string"},
                            "default": [],
                            "description": "Default empty array"
                        },
                        "optional_field": {
                            "type": "string",
                            "description": "No default value"
                        }
                    },
                    "required": ["auto_save", "max_retries", "mode"]
                }
            }
        }
    });

    println!("Testing default value patterns...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!(
            "  - {} (default: {:?}): {:?}",
            name,
            schema.default,
            match &schema.schema_type {
                openapi_to_rust::analysis::SchemaType::StringEnum { values } => {
                    format!("StringEnum({})", values.join(", "))
                }
                openapi_to_rust::analysis::SchemaType::Object { properties, .. } => {
                    let prop_info: Vec<String> = properties
                        .iter()
                        .map(|(prop_name, prop)| {
                            format!(
                                "{}: {:?} (default: {:?})",
                                prop_name, prop.schema_type, prop.default
                            )
                        })
                        .collect();
                    format!("Object({})", prop_info.join(", "))
                }
                other => format!("{other:?}"),
            }
        );
    }

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "default_values_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
