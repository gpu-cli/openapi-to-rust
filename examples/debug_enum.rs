use openapi_generator::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Debug Enum", "version": "1.0.0"},
        "components": {
            "schemas": {
                "TestEnum": {
                    "type": "string",
                    "enum": ["normal", "impl", "type"],
                    "default": "normal"
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec)?;
    let mut analysis = analyzer.analyze()?;

    let config = GeneratorConfig {
        module_name: "debug_api".to_string(),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!("Generated code:");
    println!("{generated_code}");

    Ok(())
}
