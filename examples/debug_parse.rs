use openapi_generator::openapi::Schema;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Test parsing the Model schema directly
    let model_schema_json = serde_json::json!({
        "type": null,
        "title": "Model",
        "description": "The model that will complete your prompt.",
        "anyOf": [
            { "type": "string" },
            {
                "const": "claude-3-7-sonnet-latest",
                "description": "High-performance model"
            }
        ]
    });

    println!("Attempting to parse Model schema...");
    let schema: Schema = serde_json::from_value(model_schema_json)?;

    match schema {
        Schema::AnyOf {
            any_of,
            schema_type,
            ..
        } => {
            println!(
                "✅ Parsed as AnyOf with {} variants, type: {:?}",
                any_of.len(),
                schema_type
            );
        }
        Schema::Typed { schema_type, .. } => {
            println!("❌ Parsed as Typed with type: {schema_type:?}");
        }
        _ => {
            println!(
                "❓ Parsed as other variant: {:?}",
                std::mem::discriminant(&schema)
            );
        }
    }

    Ok(())
}
