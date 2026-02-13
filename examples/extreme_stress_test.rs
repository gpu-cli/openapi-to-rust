#![recursion_limit = "512"]

use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔥 Running EXTREME stress test...\n");

    // The most complex schema I can think of - combining ALL edge cases
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Extreme Stress Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "MegaComplexSchema": {
                    "$recursiveAnchor": true,
                    "type": "object",
                    "properties": {
                        // Edge case field names
                        "type": {"type": "string", "enum": ["mega"], "default": "mega"},
                        "match": {"type": "integer", "default": 42},
                        "fn": {"type": "boolean", "default": true},
                        "123field": {"type": "string", "default": "numeric"},
                        "kebab-case-field": {"type": "string"},
                        "snake_case_field": {"type": "string"},
                        "camelCaseField": {"type": "string"},
                        "PascalCaseField": {"type": "string"},
                        "with.dots.everywhere": {"type": "string"},
                        "with spaces in name": {"type": "string"},
                        "with@#$%symbols": {"type": "string"},
                        "🚀unicode-field": {"type": "string"},

                        // Complex nullable patterns
                        "nullable_complex": {
                            "anyOf": [
                                {
                                    "type": "object",
                                    "properties": {
                                        "nested": {
                                            "anyOf": [
                                                {"$recursiveRef": "#"},
                                                {"type": "null"}
                                            ]
                                        }
                                    }
                                },
                                {"type": "null"}
                            ]
                        },

                        // Deeply nested arrays with recursion
                        "mega_nested_array": {
                            "type": "array",
                            "items": {
                                "type": "array",
                                "items": {
                                    "type": "array",
                                    "items": {
                                        "anyOf": [
                                            {"$recursiveRef": "#"},
                                            {"type": "string"},
                                            {"type": "null"}
                                        ]
                                    }
                                }
                            },
                            "default": []
                        },

                        // Complex discriminated union with allOf
                        "mega_union": {
                            "oneOf": [
                                {
                                    "allOf": [
                                        {"$ref": "#/components/schemas/BaseType"},
                                        {
                                            "type": "object",
                                            "properties": {
                                                "union_type": {"type": "string", "enum": ["variant_a"]},
                                                "a_specific": {
                                                    "anyOf": [
                                                        {"type": "string"},
                                                        {"type": "integer"},
                                                        {"$recursiveRef": "#"}
                                                    ]
                                                }
                                            },
                                            "required": ["union_type"]
                                        }
                                    ]
                                },
                                {
                                    "allOf": [
                                        {"$ref": "#/components/schemas/BaseType"},
                                        {
                                            "type": "object",
                                            "properties": {
                                                "union_type": {"type": "string", "enum": ["variant_b"]},
                                                "b_specific": {
                                                    "type": "array",
                                                    "items": {"$recursiveRef": "#"}
                                                }
                                            },
                                            "required": ["union_type"]
                                        }
                                    ]
                                }
                            ],
                            "discriminator": {"propertyName": "union_type"}
                        },

                        // Extreme composition
                        "composition_madness": {
                            "allOf": [
                                {"$ref": "#/components/schemas/BaseType"},
                                {"$ref": "#/components/schemas/Timestamped"},
                                {"$ref": "#/components/schemas/WithMetadata"},
                                {
                                    "type": "object",
                                    "properties": {
                                        "final_field": {
                                            "oneOf": [
                                                {"type": "string"},
                                                {"type": "integer"},
                                                {
                                                    "type": "object",
                                                    "additionalProperties": {
                                                        "anyOf": [
                                                            {"$recursiveRef": "#"},
                                                            {"type": "string"}
                                                        ]
                                                    }
                                                }
                                            ]
                                        }
                                    }
                                }
                            ]
                        }
                    },
                    "required": ["type", "match", "fn"],
                    "additionalProperties": {
                        "anyOf": [
                            {"$recursiveRef": "#"},
                            {"type": "string"},
                            {"type": "integer"},
                            {"type": "boolean"},
                            {"type": "null"}
                        ]
                    }
                },

                "BaseType": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "base_field": {"type": "string", "default": "base"}
                    },
                    "required": ["id"]
                },

                "Timestamped": {
                    "type": "object",
                    "properties": {
                        "created_at": {"type": "string", "format": "date-time"},
                        "updated_at": {"type": "string", "format": "date-time", "nullable": true}
                    }
                },

                "WithMetadata": {
                    "type": "object",
                    "properties": {
                        "metadata": {
                            "type": "object",
                            "additionalProperties": true,
                            "properties": {
                                "version": {"type": "integer", "default": 1},
                                "tags": {
                                    "type": "array",
                                    "items": {"type": "string"},
                                    "default": []
                                }
                            }
                        }
                    }
                },

                "ExtremeEnum": {
                    "type": "string",
                    "enum": [
                        "normal",
                        "123",
                        "null",
                        "true",
                        "false",
                        "with spaces",
                        "with.dots",
                        "type",
                        "match",
                        "fn",
                        "impl"
                    ],
                    "default": "normal"
                },

                "CircularReference": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "parent": {"$ref": "#/components/schemas/CircularReference"},
                        "children": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/CircularReference"}
                        },
                        "sibling": {"$ref": "#/components/schemas/CircularSibling"}
                    },
                    "required": ["id"]
                },

                "CircularSibling": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "reference_back": {"$ref": "#/components/schemas/CircularReference"},
                        "other_sibling": {"$ref": "#/components/schemas/CircularSibling"}
                    },
                    "required": ["id"]
                }
            }
        }
    });

    println!("🔍 Analyzing extreme schema...");
    let mut analyzer = SchemaAnalyzer::new(spec)?;
    let mut analysis = analyzer.analyze()?;

    println!(
        "  ✅ {} schemas analyzed successfully",
        analysis.schemas.len()
    );
    println!(
        "  📊 Dependency graph has {} edges",
        analysis.dependencies.edges.len()
    );
    println!(
        "  🔄 {} recursive schemas detected",
        analysis.dependencies.recursive_schemas.len()
    );

    println!("\n🛠️ Generating code...");
    let config = GeneratorConfig {
        module_name: "extreme_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    let line_count = generated_code.lines().count();
    let char_count = generated_code.len();

    println!("  ✅ Code generation completed:");
    println!("    📄 {line_count} lines generated");
    println!("    🔤 {char_count} characters generated");

    // Basic sanity checks
    assert!(generated_code.contains("pub mod extreme_api"));
    assert!(generated_code.contains("use serde"));
    assert!(generated_code.contains("pub struct MegaComplexSchema"));
    assert!(generated_code.contains("pub enum ExtremeEnum"));

    // Check that problematic identifiers were sanitized
    assert!(generated_code.contains("field_123field")); // Number prefix handled
    assert!(generated_code.contains("type_")); // Reserved keyword handled
    assert!(generated_code.contains("match_")); // Reserved keyword handled
    assert!(generated_code.contains("fn_")); // Reserved keyword handled

    // Check that Box wrapping is applied for recursive types
    assert!(generated_code.contains("Box<"));

    println!("\n🎯 Extreme stress test results:");
    println!("  ✅ All edge cases handled successfully");
    println!("  ✅ Complex recursive patterns supported");
    println!("  ✅ Identifier sanitization working");
    println!("  ✅ Generated code passes basic validation");

    println!("\n🚀 EXTREME STRESS TEST PASSED! 🚀");

    Ok(())
}
