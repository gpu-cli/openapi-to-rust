use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test comprehensive nullable patterns
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Nullable Types Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"}
                    },
                    "required": ["id", "name"]
                },
                "TestObject": {
                    "type": "object",
                    "properties": {
                        "legacy_nullable_string": {
                            "type": "string",
                            "nullable": true,
                            "description": "OpenAPI 3.0 style nullable string"
                        },
                        "anyof_nullable_string": {
                            "anyOf": [
                                {"type": "string"},
                                {"type": "null"}
                            ],
                            "description": "OpenAPI 3.1 style nullable string"
                        },
                        "nullable_user_ref": {
                            "anyOf": [
                                {"$ref": "#/components/schemas/User"},
                                {"type": "null"}
                            ],
                            "description": "Nullable reference to User"
                        },
                        "legacy_nullable_number": {
                            "type": "number",
                            "nullable": true
                        },
                        "required_string": {
                            "type": "string",
                            "description": "Required non-nullable string"
                        },
                        "optional_string": {
                            "type": "string",
                            "description": "Optional non-nullable string"
                        },
                        "required_nullable_string": {
                            "type": "string",
                            "nullable": true,
                            "description": "Required but nullable string"
                        }
                    },
                    "required": ["required_string", "required_nullable_string"]
                }
            }
        }
    });

    println!("Testing nullable type patterns...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!(
            "  - {}: {:?} (nullable: {})",
            name,
            match &schema.schema_type {
                openapi_to_rust::analysis::SchemaType::Object { properties, .. } => {
                    let prop_info: Vec<String> = properties
                        .iter()
                        .map(|(prop_name, prop)| {
                            format!(
                                "{}: {:?} (nullable: {})",
                                prop_name, prop.schema_type, prop.nullable
                            )
                        })
                        .collect();
                    format!("Object({})", prop_info.join(", "))
                }
                other => format!("{other:?}"),
            },
            schema.nullable
        );
    }

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "nullable_test_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
