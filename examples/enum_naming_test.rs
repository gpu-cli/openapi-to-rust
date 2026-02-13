use openapi_generator::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Enum Naming Test", "version": "1.0.0"},
        "components": {
            "schemas": {
                "Status": {
                    "type": "string",
                    "enum": [
                        "active",
                        "INACTIVE",
                        "pending-review",
                        "in_progress",
                        "CamelCase",
                        "PascalCase",
                        "kebab-case-long",
                        "snake_case_long",
                        "with spaces",
                        "with.dots",
                        "123numeric",
                        "null",
                        "true",
                        "false",
                        "type",
                        "impl"
                    ],
                    "default": "active"
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec)?;
    let mut analysis = analyzer.analyze()?;

    let config = GeneratorConfig {
        module_name: "naming_test".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("Generated enum with PascalCase variants:");
    println!("{generated_code}");

    Ok(())
}
