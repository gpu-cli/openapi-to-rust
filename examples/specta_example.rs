use openapi_generator::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Chat API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "ChatMessage": {
                    "type": "object",
                    "description": "A chat message from a user or assistant",
                    "properties": {
                        "role": {
                            "type": "string",
                            "enum": ["user", "assistant", "system"]
                        },
                        "content": {
                            "type": "string"
                        },
                        "timestamp": {
                            "type": "integer",
                            "format": "int64"
                        }
                    },
                    "required": ["role", "content"]
                },
                "ChatRequest": {
                    "type": "object",
                    "properties": {
                        "messages": {
                            "type": "array",
                            "items": {
                                "$ref": "#/components/schemas/ChatMessage"
                            }
                        },
                        "model": {
                            "type": "string"
                        },
                        "temperature": {
                            "type": "number"
                        }
                    },
                    "required": ["messages", "model"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    // Generate with Specta support enabled
    let config = GeneratorConfig {
        enable_specta: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    println!("=== Generated Code with Specta Support ===\n");
    println!("{}", result);
}
