use openapi_generator::SchemaAnalyzer;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read the anthropic fixture
    let content = fs::read_to_string("tests/fixtures/anthropic.yml")?;
    let spec: serde_json::Value = serde_yaml::from_str(&content)?;

    println!("Creating analyzer...");
    let mut analyzer = SchemaAnalyzer::new(spec)?;

    println!("Analyzing schemas...");
    let analysis = analyzer.analyze()?;

    println!("Total schemas found: {}", analysis.schemas.len());

    // Check if Model schema exists
    if let Some(model_schema) = analysis.schemas.get("Model") {
        println!("✅ Found Model schema!");
        println!("  Type: {:?}", model_schema.schema_type);
        println!("  Dependencies: {:?}", model_schema.dependencies);
        println!("  Nullable: {}", model_schema.nullable);
        println!(
            "  Original: {}",
            serde_json::to_string_pretty(&model_schema.original)?
        );
    } else {
        println!("❌ Model schema NOT found!");

        // List all schemas that start with 'M'
        println!("Schemas starting with 'M':");
        for name in analysis.schemas.keys() {
            if name.starts_with('M') {
                println!("  - {name}");
            }
        }
    }

    // Show first few schemas for debugging
    println!("\nFirst 10 schemas:");
    for (i, name) in analysis.schemas.keys().enumerate() {
        if i >= 10 {
            break;
        }
        println!("  {}: {}", i + 1, name);
    }

    Ok(())
}
