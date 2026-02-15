use clap::{Parser, Subcommand};
use openapi_to_rust::{CodeGenerator, ConfigFile, SchemaAnalyzer};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "openapi-to-rust")]
#[command(about = "Generate Rust types and clients from OpenAPI specs")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate code from OpenAPI spec
    Generate {
        /// Path to configuration file (openapi-to-rust.toml)
        #[arg(short, long, default_value = "openapi-to-rust.toml")]
        config: PathBuf,
    },
    /// Validate configuration file without generating code
    Validate {
        /// Path to configuration file (openapi-to-rust.toml)
        #[arg(short, long, default_value = "openapi-to-rust.toml")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Validate { config } => {
            println!("📖 Validating configuration from: {}", config.display());

            // Load and validate configuration from TOML
            match ConfigFile::load(&config) {
                Ok(_config_file) => {
                    println!("✅ Configuration is valid!");
                    Ok(())
                }
                Err(e) => {
                    eprintln!("❌ Configuration validation failed:");
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Generate { config } => {
            println!("📖 Reading configuration from: {}", config.display());

            // Load configuration from TOML
            let config_file = match ConfigFile::load(&config) {
                Ok(cf) => cf,
                Err(e) => {
                    eprintln!("❌ Failed to load configuration:");
                    eprintln!("{}", e);
                    std::process::exit(1);
                }
            };

            let generator_config = config_file.into_generator_config();

            println!(
                "📄 Reading OpenAPI spec: {}",
                generator_config.spec_path.display()
            );

            // Read and parse OpenAPI spec
            let spec_content = std::fs::read_to_string(&generator_config.spec_path)?;
            let spec_value: serde_json::Value = if generator_config.spec_path.extension()
                == Some(std::ffi::OsStr::new("yaml"))
                || generator_config.spec_path.extension() == Some(std::ffi::OsStr::new("yml"))
            {
                serde_yaml::from_str(&spec_content)?
            } else {
                serde_json::from_str(&spec_content)?
            };

            // Analyze schemas (with extensions if configured)
            println!("🔍 Analyzing schemas...");
            let mut analyzer = if generator_config.schema_extensions.is_empty() {
                SchemaAnalyzer::new(spec_value)?
            } else {
                println!(
                    "📎 Merging {} schema extension(s)",
                    generator_config.schema_extensions.len()
                );
                SchemaAnalyzer::new_with_extensions(
                    spec_value,
                    &generator_config.schema_extensions,
                )?
            };
            let mut analysis = analyzer.analyze()?;

            println!("📊 Found {} schemas", analysis.schemas.len());
            println!("📊 Found {} operations", analysis.operations.len());

            // Generate code
            println!("⚙️  Generating code...");
            let generator = CodeGenerator::new(generator_config);
            let result = generator.generate_all(&mut analysis)?;

            // Write files
            generator.write_files(&result)?;

            println!(
                "✅ Generated {} files to {}",
                result.files.len(),
                generator.config().output_dir.display()
            );

            Ok(())
        }
    }
}
