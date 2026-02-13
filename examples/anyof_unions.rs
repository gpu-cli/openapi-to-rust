use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec with anyOf patterns
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "AnyOf Union Test API",
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
                "Organization": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "company_name": {
                            "type": "string"
                        }
                    },
                    "required": ["id", "company_name"]
                },
                "NullableUser": {
                    "anyOf": [
                        {"$ref": "#/components/schemas/User"},
                        {"type": "null"}
                    ],
                    "description": "A user that can be null"
                },
                "UserOrOrganization": {
                    "anyOf": [
                        {"$ref": "#/components/schemas/User"},
                        {"$ref": "#/components/schemas/Organization"}
                    ],
                    "description": "Either a user or an organization"
                },
                "FlexibleId": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "integer"}
                    ],
                    "description": "ID that can be string or number"
                },
                "ComplexAnyOf": {
                    "anyOf": [
                        {"$ref": "#/components/schemas/User"},
                        {"$ref": "#/components/schemas/Organization"},
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "enum": ["guest"]
                                },
                                "session_id": {
                                    "type": "string"
                                }
                            },
                            "required": ["type", "session_id"]
                        }
                    ],
                    "description": "User, organization, or guest session"
                },
                "ApiResponse": {
                    "type": "object",
                    "properties": {
                        "success": {
                            "type": "boolean"
                        },
                        "data": {
                            "$ref": "#/components/schemas/UserOrOrganization"
                        },
                        "error": {
                            "$ref": "#/components/schemas/NullableUser"
                        }
                    },
                    "required": ["success"]
                }
            }
        }
    });

    println!("Creating schema analyzer for anyOf union test...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!(
            "  - {}: {:?}",
            name,
            match &schema.schema_type {
                openapi_to_rust::analysis::SchemaType::Union { variants } => {
                    format!("Union(variants: {})", variants.len())
                }
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
        module_name: "anyof_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
