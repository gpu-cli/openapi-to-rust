use openapi_generator::SchemaAnalyzer;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Read the anthropic spec
    let content = fs::read_to_string("../anthropic/anthropic.json")?;
    let spec: serde_json::Value = serde_json::from_str(&content)?;

    println!("Creating analyzer...");
    let mut analyzer = SchemaAnalyzer::new(spec)?;

    println!("Analyzing schemas...");
    let analysis = analyzer.analyze()?;

    // Check BetaInputContentBlock
    if let Some(schema) = analysis.schemas.get("BetaInputContentBlock") {
        println!("✅ Found BetaInputContentBlock schema!");
        println!("Schema type: {:#?}", schema.schema_type);

        // Print the variants if it's a discriminated union
        if let openapi_generator::analysis::SchemaType::DiscriminatedUnion {
            discriminator_field,
            variants,
        } = &schema.schema_type
        {
            println!("\nDiscriminator field: {discriminator_field}");
            println!("Variants ({}):", variants.len());
            for (i, variant) in variants.iter().enumerate() {
                println!(
                    "  {}. rust_name: {}, type_name: {}, discriminator_value: {}",
                    i + 1,
                    variant.rust_name,
                    variant.type_name,
                    variant.discriminator_value
                );
            }
        }
    } else {
        println!("❌ BetaInputContentBlock schema NOT found!");
    }

    Ok(())
}
