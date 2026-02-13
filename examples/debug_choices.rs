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

    // Find the CreateChatCompletionStreamResponse struct
    let lines: Vec<&str> = result.lines().collect();
    let mut in_stream_response = false;

    for (i, line) in lines.iter().enumerate() {
        if line.contains("struct CreateChatCompletionStreamResponse") {
            in_stream_response = true;
            println!("Found CreateChatCompletionStreamResponse at line {}", i);
            println!("\nFull struct definition:");
        }

        if in_stream_response {
            println!("{}", line);

            if line.contains("pub choices:") {
                println!("\n>>> CHOICES FIELD TYPE: {}", line);
            }

            if *line == "}" {
                in_stream_response = false;
            }
        }
    }

    // Also check if there's a separate type for the choice items
    println!("\nSearching for related choice types:");
    for line in &lines {
        if line.contains("Choice")
            && (line.contains("struct") || line.contains("enum") || line.contains("type"))
        {
            println!("{}", line);
        }
    }

    Ok(())
}
