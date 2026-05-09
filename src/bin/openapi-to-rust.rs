use clap::{Parser, Subcommand};
use openapi_to_rust::cli::{json_from_str_lossy, yaml_to_json_value};
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
                yaml_to_json_value(&spec_content)?
            } else {
                json_from_str_lossy(&spec_content)?
            };

            // Version gate: surface unsupported OAS major.minor early.
            let oas_version = spec_value
                .get("openapi")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match openapi_to_rust::cli::parse_oas_version(oas_version) {
                Some((3, 0)) | Some((3, 1)) => {}
                Some((3, 2)) => {
                    eprintln!("⚠️  OpenAPI {oas_version}: 3.2 is experimentally supported.");
                }
                Some((major, minor)) => {
                    eprintln!(
                        "❌ Unsupported OpenAPI version: {major}.{minor} ({oas_version:?}). \
                         This generator targets 3.0.x, 3.1.x, and (experimentally) 3.2.x. \
                         Swagger 2.0 and OAS 1.x are not supported."
                    );
                    std::process::exit(1);
                }
                None => {
                    let hint = if spec_value.get("swagger").is_some() {
                        " (looks like Swagger 2.0 — out of scope)"
                    } else {
                        ""
                    };
                    eprintln!(
                        "❌ Missing or unrecognised `openapi` field{hint}. Expected something like \"3.1.0\", got: {oas_version:?}"
                    );
                    std::process::exit(1);
                }
            }

            // Analyze schemas (with extensions if configured). Build a
            // TypeMapper from the user's [generator.types] config so
            // per-format strategies drive type generation (Q2.0).
            println!("🔍 Analyzing schemas...");
            let type_mapper =
                openapi_to_rust::TypeMapper::new(generator_config.types.clone());
            let mut analyzer = if generator_config.schema_extensions.is_empty() {
                SchemaAnalyzer::with_type_mapper(spec_value, type_mapper)?
            } else {
                println!(
                    "📎 Merging {} schema extension(s)",
                    generator_config.schema_extensions.len()
                );
                SchemaAnalyzer::new_with_extensions_and_type_mapper(
                    spec_value,
                    &generator_config.schema_extensions,
                    type_mapper,
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
