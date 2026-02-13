use crate::{CodeGenerator, GeneratorConfig, SchemaAnalyzer, streaming::StreamingConfig};
use clap::{Arg, Command};
use std::fs;
use std::process;

/// Configuration for the CLI helper
pub struct CliConfig {
    /// Name of the API for display purposes
    pub api_name: &'static str,
    /// Default module name
    pub default_module_name: &'static str,
    /// Streaming configuration for this API
    pub streaming_config: Option<StreamingConfig>,
    /// Enable Specta type derives for frontend integration
    pub enable_specta: bool,
}

/// Run the complete generation CLI with the provided configuration
pub async fn run_generation_cli(cli_config: CliConfig) {
    let matches = Command::new("api-gen")
        .version("0.1.0")
        .about("Generate API types and streaming client")
        .arg(
            Arg::new("input")
                .help("Input OpenAPI spec (file path or URL)")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("output-dir")
                .long("output-dir")
                .value_name("DIR")
                .help("Output directory for generated files (default: src/generated)")
                .default_value("src/generated"),
        )
        .arg(
            Arg::new("module-name")
                .short('m')
                .long("module-name")
                .value_name("NAME")
                .help("Generated module name")
                .default_value(cli_config.default_module_name),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Enable verbose output")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .help("Print generated code to stdout instead of writing to file")
                .action(clap::ArgAction::SetTrue),
        )
        .get_matches();

    let Some(input) = matches.get_one::<String>("input") else {
        eprintln!("Error: missing required argument 'input'");
        process::exit(1);
    };
    let Some(output_dir) = matches.get_one::<String>("output-dir") else {
        eprintln!("Error: missing required argument 'output-dir'");
        process::exit(1);
    };
    let Some(module_name) = matches.get_one::<String>("module-name") else {
        eprintln!("Error: missing required argument 'module-name'");
        process::exit(1);
    };
    let verbose = matches.get_flag("verbose");
    let dry_run = matches.get_flag("dry-run");

    if verbose {
        println!("🚀 {} API Generator", cli_config.api_name);
        println!("Input: {input}");
        if !dry_run {
            println!("Output: {output_dir}");
        }
        println!("Module: {module_name}");
        if cli_config.streaming_config.is_some() {
            println!("🌊 Streaming: enabled");
        }
        println!();
    }

    // Load OpenAPI spec
    let spec_content = match load_spec(input, verbose).await {
        Ok(content) => content,
        Err(e) => {
            eprintln!("❌ Error loading spec: {e}");
            process::exit(1);
        }
    };

    if verbose {
        println!("📄 Loaded OpenAPI spec ({} bytes)", spec_content.len());
    }

    // Parse the spec
    let spec_value: serde_json::Value = match parse_spec(&spec_content, input) {
        Ok(value) => value,
        Err(e) => {
            eprintln!("❌ Error parsing spec: {e}");
            process::exit(1);
        }
    };

    if verbose {
        if let Some(info) = spec_value.get("info") {
            if let Some(title) = info.get("title").and_then(|t| t.as_str()) {
                println!("📋 Title: {title}");
            }
            if let Some(version) = info.get("version").and_then(|v| v.as_str()) {
                println!("🏷️  Version: {version}");
            }
        }
        println!();
    }

    // Analyze schemas
    if verbose {
        println!("🔍 Analyzing schemas...");
    }

    let mut analyzer = match SchemaAnalyzer::new(spec_value) {
        Ok(analyzer) => analyzer,
        Err(e) => {
            eprintln!("❌ Error creating analyzer: {e}");
            process::exit(1);
        }
    };

    let mut analysis = match analyzer.analyze() {
        Ok(analysis) => analysis,
        Err(e) => {
            eprintln!("❌ Error analyzing schemas: {e}");
            process::exit(1);
        }
    };

    if verbose {
        println!("📈 Found {} schemas", analysis.schemas.len());
        println!("📈 Found {} operations", analysis.operations.len());
        if let Some(ref config) = cli_config.streaming_config {
            println!("🌊 Found {} streaming endpoints", config.endpoints.len());
        }
        println!();
    }

    // Generate code
    if verbose {
        let stream_status = if cli_config.streaming_config.is_some() {
            "with streaming support"
        } else {
            ""
        };
        println!(
            "⚙️  Generating {} API code {}...",
            cli_config.api_name, stream_status
        );
    }

    let config = GeneratorConfig {
        module_name: module_name.clone(),
        output_dir: output_dir.into(),
        streaming_config: cli_config.streaming_config,
        enable_specta: cli_config.enable_specta,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generation_result = match generator.generate_all(&mut analysis) {
        Ok(result) => result,
        Err(e) => {
            eprintln!("❌ Error generating code: {e}");
            process::exit(1);
        }
    };

    if dry_run {
        println!("=== Generated Files ===");
        for file in &generation_result.files {
            println!("\n--- {} ---", file.path.display());
            println!("{}", file.content);
        }
        println!("\n--- {} ---", generation_result.mod_file.path.display());
        println!("{}", generation_result.mod_file.content);
    } else {
        // Write all files to disk
        if let Err(e) = generator.write_files(&generation_result) {
            eprintln!("❌ Error writing files: {e}");
            process::exit(1);
        }

        if verbose {
            println!(
                "✅ Generated {} files written to: {}",
                generation_result.files.len() + 1,
                generator.config().output_dir.display()
            );
            for file in &generation_result.files {
                println!("   - {}", file.path.display());
            }
            println!("   - {}", generation_result.mod_file.path.display());
        } else {
            println!(
                "✅ Generated {} files written to: {}",
                generation_result.files.len() + 1,
                generator.config().output_dir.display()
            );
        }
    }
}

async fn load_spec(input: &str, verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    if input.starts_with("http://") || input.starts_with("https://") {
        // Load from URL
        if verbose {
            println!("🌐 Fetching from URL...");
        }

        let response = reqwest::get(input).await?;
        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()).into());
        }

        let content = response.text().await?;
        Ok(content)
    } else {
        // Load from file
        if verbose {
            println!("📁 Reading from file...");
        }

        let content = fs::read_to_string(input)?;
        Ok(content)
    }
}

fn parse_spec(content: &str, input: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Determine format from extension or content
    let is_yaml = input.ends_with(".yaml")
        || input.ends_with(".yml")
        || content.trim_start().starts_with("openapi:")
        || content.trim_start().starts_with("swagger:");

    if is_yaml {
        let value: serde_json::Value = serde_yaml::from_str(content)?;
        Ok(value)
    } else {
        let value: serde_json::Value = serde_json::from_str(content)?;
        Ok(value)
    }
}
