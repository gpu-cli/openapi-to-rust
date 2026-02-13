use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create an OpenAPI spec with recursive schemas
    let test_spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Recursive Schemas Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "TreeNode": {
                    "type": "object",
                    "properties": {
                        "value": {
                            "type": "string"
                        },
                        "children": {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/TreeNode"
                            }
                        },
                        "parent": {
                            "$ref": "#/components/schemas/TreeNode"
                        }
                    },
                    "required": ["value"],
                    "description": "A tree node with children and parent references"
                },
                "LinkedListNode": {
                    "type": "object",
                    "properties": {
                        "data": {
                            "type": "integer"
                        },
                        "next": {
                            "$ref": "#/components/schemas/LinkedListNode"
                        }
                    },
                    "required": ["data"],
                    "description": "A linked list node with next pointer"
                },
                "Person": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "age": {
                            "type": "integer"
                        },
                        "friends": {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/Person"
                            }
                        },
                        "spouse": {
                            "$ref": "#/components/schemas/Person"
                        }
                    },
                    "required": ["name", "age"],
                    "description": "A person with friends and spouse (mutual references)"
                },
                "Category": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "parent": {
                            "$ref": "#/components/schemas/Category"
                        },
                        "subcategories": {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/Category"
                            }
                        }
                    },
                    "required": ["name"],
                    "description": "A category with parent and child categories"
                },
                "GraphNode": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "edges": {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/GraphEdge"
                            }
                        }
                    },
                    "required": ["id"]
                },
                "GraphEdge": {
                    "type": "object",
                    "properties": {
                        "from": {
                            "$ref": "#/components/schemas/GraphNode"
                        },
                        "to": {
                            "$ref": "#/components/schemas/GraphNode"
                        },
                        "weight": {
                            "type": "number"
                        }
                    },
                    "required": ["from", "to"],
                    "description": "Mutual recursion between GraphNode and GraphEdge"
                }
            }
        }
    });

    println!("Creating schema analyzer for recursive schemas test...");
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
                    let prop_info: Vec<String> = properties
                        .iter()
                        .map(|(prop_name, prop_info)| match &prop_info.schema_type {
                            openapi_to_rust::analysis::SchemaType::Reference { target } => {
                                format!("{prop_name}: {target}")
                            }
                            openapi_to_rust::analysis::SchemaType::Array { item_type } => {
                                match item_type.as_ref() {
                                    openapi_to_rust::analysis::SchemaType::Reference {
                                        target,
                                    } => {
                                        format!("{prop_name}: Vec<{target}>")
                                    }
                                    _ => format!("{prop_name}: {item_type:?}"),
                                }
                            }
                            _ => format!("{}: {:?}", prop_name, prop_info.schema_type),
                        })
                        .collect();
                    format!("Object({})", prop_info.join(", "))
                }
                other => format!("{other:?}"),
            }
        );
    }

    println!("\nDependency graph:");
    for (schema, deps) in &analysis.dependencies.edges {
        if !deps.is_empty() {
            println!("  {schema} depends on: {deps:?}");
        }
    }

    println!(
        "\nRecursive schemas detected: {:?}",
        analysis.dependencies.recursive_schemas
    );

    println!("\nGenerating code...");
    let config = GeneratorConfig {
        module_name: "recursive_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("\nGenerated code:");
    println!("{generated_code}");

    Ok(())
}
