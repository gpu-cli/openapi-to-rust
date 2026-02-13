use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "User API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "firstName": {"type": "string"},
                        "lastName": {"type": "string"},
                        "email": {"type": "string"}
                    },
                    "required": ["id"]
                }
            }
        }
    });

    // Generate WITHOUT Specta
    {
        let mut analyzer = SchemaAnalyzer::new(spec.clone()).expect("Failed to create analyzer");
        let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

        let config = GeneratorConfig {
            enable_specta: false,
            ..Default::default()
        };

        let generator = CodeGenerator::new(config);
        let result = generator
            .generate(&mut analysis)
            .expect("Failed to generate code");

        println!("=== WITHOUT Specta (enable_specta: false) ===\n");
        // Extract just the User struct
        if let Some(start) = result.find("pub struct User") {
            if let Some(end) = result[start..].find('}') {
                println!("{}", &result[start..start + end + 1]);
            }
        }
    }

    println!("\n");

    // Generate WITH Specta
    {
        let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
        let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

        let config = GeneratorConfig {
            enable_specta: true,
            ..Default::default()
        };

        let generator = CodeGenerator::new(config);
        let result = generator
            .generate(&mut analysis)
            .expect("Failed to generate code");

        println!("=== WITH Specta (enable_specta: true) ===\n");
        // Extract just the User struct
        if let Some(start) = result.find("pub struct User") {
            let lines_before = &result[..start];
            let attr_start = lines_before.rfind("#[").unwrap_or(start);
            if let Some(end) = result[start..].find('}') {
                println!("{}", &result[attr_start..start + end + 1]);
            }
        }
    }

    println!("\n");
    println!("Key Differences:");
    println!("1. WITH Specta adds: #[cfg_attr(feature = \"specta\", derive(specta::Type))]");
    println!(
        "2. WITH Specta adds: #[cfg_attr(feature = \"specta\", specta(rename_all = \"camelCase\"))]"
    );
    println!("3. WITH Specta adds: #[serde(rename_all = \"camelCase\")] for consistent naming");
    println!("4. Backward compatible: specta is only active when 'specta' feature is enabled");
}
