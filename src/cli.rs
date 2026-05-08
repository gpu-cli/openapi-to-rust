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

/// Parse the `openapi` version string into (major, minor). Tolerates patch and
/// build-metadata suffixes. Returns None for unrecognised input.
fn parse_oas_version(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor_raw = parts.next()?;
    let minor_digits: String = minor_raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    let minor = minor_digits.parse().ok()?;
    Some((major, minor))
}

fn parse_spec(content: &str, input: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Determine format from extension or content
    let is_yaml = input.ends_with(".yaml")
        || input.ends_with(".yml")
        || content.trim_start().starts_with("openapi:")
        || content.trim_start().starts_with("swagger:");

    if is_yaml {
        let value = yaml_to_json_value(content)?;
        Ok(value)
    } else {
        let value = json_from_str_lossy(content)?;
        Ok(value)
    }
}

/// Parse YAML to serde_json::Value, converting large numbers to f64 to avoid overflow.
/// serde_yaml 0.9 cannot represent integers exceeding i64/u64 range (e.g. numbers > 2^64),
/// so we preprocess the YAML to convert such numbers to float notation, then go through
/// serde_yaml::Value and convert to serde_json::Value manually.
pub fn yaml_to_json_value(content: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let preprocessed = sanitize_large_yaml_integers(content);
    let yaml_value: serde_yaml::Value = serde_yaml::from_str(&preprocessed)?;
    Ok(yaml_value_to_json(yaml_value))
}

/// Parse JSON with lossy number handling: numbers that overflow i64/u64 are stored as f64.
pub fn json_from_str_lossy(content: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    // Try normal parsing first (fast path)
    match serde_json::from_str::<serde_json::Value>(content) {
        Ok(v) => Ok(v),
        Err(e) => {
            let err_msg = e.to_string();
            if err_msg.contains("number out of range") {
                // Fall back: parse via YAML which handles large numbers
                let yaml_value: serde_yaml::Value = serde_yaml::from_str(content)?;
                Ok(yaml_value_to_json(yaml_value))
            } else {
                Err(e.into())
            }
        }
    }
}

fn yaml_value_to_json(yaml: serde_yaml::Value) -> serde_json::Value {
    match yaml {
        serde_yaml::Value::Null => serde_json::Value::Null,
        serde_yaml::Value::Bool(b) => serde_json::Value::Bool(b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_json::Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::json!(f)
            } else {
                // Fallback: represent as 0.0
                serde_json::json!(0.0)
            }
        }
        serde_yaml::Value::String(s) => serde_json::Value::String(s),
        serde_yaml::Value::Sequence(seq) => {
            serde_json::Value::Array(seq.into_iter().map(yaml_value_to_json).collect())
        }
        serde_yaml::Value::Mapping(map) => {
            let obj = map
                .into_iter()
                .filter_map(|(k, v)| {
                    let key = match k {
                        serde_yaml::Value::String(s) => s,
                        serde_yaml::Value::Number(n) => n.to_string(),
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((key, yaml_value_to_json(v)))
                })
                .collect();
            serde_json::Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_value_to_json(tagged.value),
    }
}

/// Preprocess YAML content to convert integers that exceed i64/u64 range to float notation.
/// serde_yaml 0.9 cannot parse integers larger than u64::MAX or smaller than i64::MIN,
/// so we find bare integer values on YAML lines and append `.0` if they overflow.
fn sanitize_large_yaml_integers(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    for line in content.lines() {
        if let Some(sanitized) = try_sanitize_integer_line(line) {
            result.push_str(&sanitized);
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }
    result
}

/// If a YAML line has a `key: <integer>` pattern where the integer overflows i64/u64,
/// convert it to float by appending `.0`. Returns None if no change needed.
fn try_sanitize_integer_line(line: &str) -> Option<String> {
    // Match pattern: optional whitespace, key, colon, space(s), then a number value
    // We look for the value portion after the last `: ` or `- ` on the line
    let trimmed = line.trim();

    // Skip comments and empty lines
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    // Find the value part — after `: ` for mapping entries
    let colon_pos = line.find(": ")?;
    let value_start = colon_pos + 2;
    let value_str = line[value_start..].trim();

    // Check if the value looks like a bare integer (optional leading minus, then digits)
    if value_str.is_empty() {
        return None;
    }

    let (is_negative, digit_part) = if let Some(rest) = value_str.strip_prefix('-') {
        (true, rest)
    } else {
        (false, value_str)
    };

    // Must be all digits
    if !digit_part.chars().all(|c| c.is_ascii_digit()) || digit_part.is_empty() {
        return None;
    }

    // Check if it overflows i64/u64
    let overflows = if is_negative {
        // Check if |value| > i64::MAX + 1 = 9223372036854775808
        digit_part.len() > 19 || (digit_part.len() == 19 && digit_part > "9223372036854775808")
    } else {
        // Check if value > u64::MAX = 18446744073709551615
        digit_part.len() > 20 || (digit_part.len() == 20 && digit_part > "18446744073709551615")
    };

    if overflows {
        // Replace the integer with float notation
        let mut sanitized = line[..value_start].to_string();
        sanitized.push_str(value_str);
        sanitized.push_str(".0");
        Some(sanitized)
    } else {
        None
    }
}
