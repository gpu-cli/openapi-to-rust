use openapi_generator::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load the chat-completions.json file
    let spec_path = Path::new("../chat-completions-client/chat-completions.json");
    let spec_content = std::fs::read_to_string(spec_path)?;
    let spec_value: serde_json::Value = serde_json::from_str(&spec_content)?;

    // Analyze schemas
    let mut analyzer = SchemaAnalyzer::new(spec_value)?;
    let mut analysis = analyzer.analyze()?;

    // Create config
    let config = GeneratorConfig::default();

    // Generate types
    let generator = CodeGenerator::new(config);
    let result = generator.generate(&mut analysis)?;

    // Search for CreateChatCompletionStreamResponse
    if let Some(start) = result.find("pub struct CreateChatCompletionStreamResponse") {
        if let Some(end) = result[start..].find("\n}").map(|e| start + e + 2) {
            println!("CreateChatCompletionStreamResponse struct:");
            println!("{}", &result[start..end]);
        }
    }

    // Search for the generated item type - look for any struct with "Item" suffix
    println!("\n\nSearching for Item structs:");
    for line in result.lines() {
        if line.contains("pub struct")
            && line.contains("Item")
            && line.contains("CreateChatCompletionStreamResponse")
        {
            println!("Found: {}", line);
        }
    }

    // Try to find the exact generated item struct
    if let Some(start) = result.find("pub struct CreateChatCompletionStreamResponseChoicesItem") {
        if let Some(end) = result[start..].find("\n}").map(|e| start + e + 2) {
            println!("\nCreateChatCompletionStreamResponseChoicesItem struct:");
            println!("{}", &result[start..end]);
        }
    }

    Ok(())
}
