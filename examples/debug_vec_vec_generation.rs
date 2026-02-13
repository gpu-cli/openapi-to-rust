use serde_json::json;

fn main() {
    // Minimal test case for Vec<Vec<i64>> generation
    let spec = json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Debug API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "TestRequest": {
                    "type": "object",
                    "properties": {
                        "data": {
                            "oneOf": [
                                {
                                    "type": "string"
                                },
                                {
                                    "type": "array",
                                    "items": {
                                        "type": "array",
                                        "items": {
                                            "type": "integer"
                                        }
                                    }
                                }
                            ]
                        }
                    }
                }
            }
        }
    });

    // Generate code
    let mut analyzer = openapi_generator::SchemaAnalyzer::new(spec).unwrap();
    let mut analysis = analyzer.analyze().unwrap();

    println!("Analysis result:");
    for (name, schema) in &analysis.schemas {
        println!("Schema: {name}");
        println!("  Type: {:?}", schema.schema_type);
        println!("  Dependencies: {:?}", schema.dependencies);
    }

    let generator = openapi_generator::CodeGenerator::new(Default::default());
    let generated = generator.generate(&mut analysis).unwrap();

    println!("\nGenerated code:\n{generated}");
}
