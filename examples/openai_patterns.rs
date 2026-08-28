use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test patterns found in openai.json that might not be handled
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "OpenAI Patterns Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "ConstantEnum": {
                    "type": "string",
                    "enum": ["approximate"],
                    "default": "approximate",
                    "x-stainless-const": true,
                    "description": "Single-value enum with constant extension"
                },
                "MultiValueEnum": {
                    "type": "string",
                    "enum": ["left", "right", "wheel", "back", "forward"],
                    "default": "left",
                    "description": "Multi-value enum with default"
                },
                "MixedNullable": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "null"}
                    ],
                    "description": "OpenAPI 3.1 style nullable"
                },
                "LegacyNullable": {
                    "type": "string",
                    "nullable": true,
                    "description": "OpenAPI 3.0 style nullable"
                },
                "ValidationConstraints": {
                    "type": "object",
                    "properties": {
                        "shortString": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 64
                        },
                        "largeString": {
                            "type": "string",
                            "maxLength": 10485760
                        },
                        "boundedNumber": {
                            "type": "number",
                            "minimum": 0,
                            "maximum": 1
                        },
                        "boundedArray": {
                            "type": "array",
                            "items": {"type": "string"},
                            "minItems": 1,
                            "maxItems": 10
                        }
                    },
                    "additionalProperties": false
                },
                "PermissiveObject": {
                    "type": "object",
                    "properties": {
                        "knownField": {"type": "string"}
                    },
                    "additionalProperties": true
                },
                "RecursiveWithAnchor": {
                    "$recursiveAnchor": true,
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "children": {
                            "type": "array",
                            "items": {"$recursiveRef": "#"}
                        }
                    }
                }
            }
        }
    });

    println!("Testing OpenAI-specific patterns...");
    let mut analyzer = SchemaAnalyzer::new(test_spec)?;

    println!("Analyzing schemas...");
    let mut analysis = analyzer.analyze()?;

    println!("Found {} schemas:", analysis.schemas.len());
    for (name, schema) in &analysis.schemas {
        println!(
            "  - {}: {:?}",
            name,
            match &schema.schema_type {
                openapi_to_rust::analysis::SchemaType::StringEnum { values } => {
                    format!("StringEnum({})", values.join(", "))
                }
                openapi_to_rust::analysis::SchemaType::Object { properties, .. } => {
                    format!("Object({} properties)", properties.len())
                }
                openapi_to_rust::analysis::SchemaType::Union { variants, .. } => {
                    let variant_info: Vec<String> =
                        variants.iter().map(|v| v.target.to_string()).collect();
                    format!("Union(variants: [{}])", variant_info.join(", "))
                }
                other => format!("{other:?}"),
            }
        );
    }

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "openai_patterns_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
