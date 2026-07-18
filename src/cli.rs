use crate::spec_source::{is_remote_spec, parse_oas_version, parse_spec, validate_remote_spec_url};
use crate::{CodeGenerator, GeneratorConfig, SchemaAnalyzer, streaming::StreamingConfig};
use clap::{Arg, Command};
use std::fs;
use std::io::Read;
use std::process;
use std::time::Duration;

/// Maximum accepted size for a remotely fetched OpenAPI document (64 MiB).
pub const MAX_REMOTE_SPEC_BYTES: u64 = 64 * 1024 * 1024;
/// End-to-end timeout for fetching a remote OpenAPI document.
pub const REMOTE_SPEC_TIMEOUT: Duration = Duration::from_secs(30);

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

/// Run the legacy embedded generation CLI with the provided configuration.
#[deprecated(note = "use the openapi-to-rust binary's generate command")]
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
    let spec_content = match load_spec(input) {
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

    // Version gate: surface unsupported OAS major.minor early. We support
    // 3.0.x and 3.1.x first-class; 3.2.x parses (extensions on the spec model
    // capture the new fields), but several 3.2-only behaviors are still TODO,
    // so warn loudly. See https://github.com/gpu-cli/openapi-to-rust/issues/14.
    let oas_version = spec_value
        .get("openapi")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if let Some((major, minor)) = parse_oas_version(oas_version) {
        match (major, minor) {
            (3, 0) | (3, 1) => {}
            (3, 2) => {
                eprintln!(
                    "⚠️  OpenAPI {oas_version}: 3.2 is experimentally supported. \
                     Some 3.2-only features (additionalOperations, query method, \
                     itemSchema, $self, defaultMapping) are not yet wired through codegen. \
                     See issue #14 for status."
                );
            }
            _ => {
                eprintln!(
                    "❌ Unsupported OpenAPI version: {oas_version}. \
                     This generator targets 3.0.x, 3.1.x, and (experimentally) 3.2.x."
                );
                process::exit(1);
            }
        }
    } else {
        eprintln!(
            "❌ Missing or unrecognised `openapi` field. Expected something like \"3.1.0\", got: {oas_version:?}"
        );
        process::exit(1);
    }

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

/// Load a local or remote OpenAPI document with bounded remote I/O.
///
/// HTTPS is accepted everywhere. Plain HTTP is accepted only for loopback
/// hosts so local development and deterministic integration tests do not need
/// a certificate while non-local traffic remains encrypted.
pub fn load_spec(input: &str) -> Result<String, Box<dyn std::error::Error>> {
    if !is_remote_spec(input) {
        if input.contains("://") {
            return match validate_remote_spec_url(input) {
                Ok(_) => Err("unsupported remote OpenAPI URL".into()),
                Err(error) => Err(error.into()),
            };
        }
        return Ok(fs::read_to_string(input)?);
    }

    let url = validate_remote_spec_url(input)?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(REMOTE_SPEC_TIMEOUT)
        // Redirects are intentionally disabled: an allowed HTTPS URL could
        // otherwise redirect to disallowed plaintext transport.
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("openapi-to-rust/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client.get(url).send().map_err(|error| {
        format!(
            "failed to fetch remote OpenAPI document: {}",
            error.without_url()
        )
    })?;
    if !response.status().is_success() {
        return Err(format!(
            "failed to fetch OpenAPI document: HTTP {}",
            response.status()
        )
        .into());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REMOTE_SPEC_BYTES)
    {
        return Err(format!(
            "remote OpenAPI document exceeds the {} MiB response-size limit",
            MAX_REMOTE_SPEC_BYTES / 1024 / 1024
        )
        .into());
    }

    let mut bytes = Vec::new();
    response
        .take(MAX_REMOTE_SPEC_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REMOTE_SPEC_BYTES {
        return Err(format!(
            "remote OpenAPI document exceeds the {} MiB response-size limit",
            MAX_REMOTE_SPEC_BYTES / 1024 / 1024
        )
        .into());
    }
    Ok(String::from_utf8(bytes)?)
}
