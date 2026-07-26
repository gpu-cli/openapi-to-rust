use clap::{Parser, Subcommand};
use openapi_to_rust::cli::load_spec;
use openapi_to_rust::server::{
    OperationIndex, Selector,
    edit::Editor as ServerEditor,
    list::{ListFilter, ListOutput, render as render_list},
    resolve as resolve_selectors,
};
use openapi_to_rust::spec_source::{parse_spec, sanitize_source_provenance};
use openapi_to_rust::{CodeGenerator, ConfigFile, GeneratorConfig, SchemaAnalyzer};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "openapi-to-rust")]
#[command(
    about = "Generate typed Rust models, HTTP/SSE clients, and Axum servers from OpenAPI specs"
)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate code from OpenAPI spec
    Generate {
        /// Local OpenAPI file or HTTPS URL. Omit to use config mode.
        source: Option<String>,
        /// Path to configuration file. Defaults to openapi-to-rust.toml when
        /// no positional source is supplied.
        #[arg(short, long, conflicts_with = "source")]
        config: Option<PathBuf>,
        /// Direct-mode output directory.
        #[arg(long, requires = "source")]
        output_dir: Option<PathBuf>,
        /// Direct-mode generated module label.
        #[arg(long, requires = "source")]
        module_name: Option<String>,
        /// Direct mode: emit model types without an HTTP client.
        #[arg(long, requires = "source")]
        types_only: bool,
        /// Force every typed-scalar strategy back to "string" (Q2).
        /// Useful for bisecting regressions caused by typed-scalar
        /// adoption — overrides any `[generator.types]` settings in
        /// the TOML config.
        #[arg(long)]
        types_conservative: bool,
        /// Analyze and render in memory without writing files.
        #[arg(long, conflicts_with = "check")]
        dry_run: bool,
        /// Exit unsuccessfully when generated files are missing or stale.
        #[arg(long, conflicts_with = "dry_run")]
        check: bool,
        /// Suppress successful human-readable output.
        #[arg(long, conflicts_with = "json")]
        quiet: bool,
        /// Emit one machine-readable JSON result on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Create a valid starter openapi-to-rust.toml.
    Init {
        /// Local OpenAPI file or HTTPS URL.
        source: String,
        /// Starter configuration path.
        #[arg(short, long, default_value = "openapi-to-rust.toml")]
        config: PathBuf,
        /// Output directory recorded in the starter config.
        #[arg(long, default_value = "src/generated")]
        output_dir: PathBuf,
        /// Generated module label (derived from the source when omitted).
        #[arg(long)]
        module_name: Option<String>,
        /// Configure model generation without an HTTP client.
        #[arg(long)]
        types_only: bool,
        /// Replace an existing configuration file.
        #[arg(long)]
        force: bool,
        /// Validate and print/describe the starter config without writing it.
        #[arg(long)]
        dry_run: bool,
        /// Suppress successful human-readable output.
        #[arg(long, conflicts_with = "json")]
        quiet: bool,
        /// Emit one machine-readable JSON result on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Validate configuration file without generating code
    Validate {
        /// Path to configuration file (openapi-to-rust.toml)
        #[arg(short, long, default_value = "openapi-to-rust.toml")]
        config: PathBuf,
    },
    /// Server codegen commands (opt-in Axum scaffolding).
    Server {
        #[command(subcommand)]
        action: ServerCommands,
    },
}

#[derive(Subcommand)]
enum ServerCommands {
    /// List every operation in a spec. Read-only.
    List {
        /// Path to the OpenAPI spec (.yaml/.yml/.json). If omitted,
        /// the spec_path from openapi-to-rust.toml is used.
        #[arg(long)]
        spec: Option<PathBuf>,
        /// Path to TOML config to read spec_path from when --spec is absent.
        #[arg(long, default_value = "openapi-to-rust.toml")]
        config: PathBuf,
        /// Substring match against tag names (case insensitive).
        #[arg(long)]
        tag: Option<String>,
        /// Exact HTTP method filter (GET, POST, ...; case insensitive).
        #[arg(long)]
        method: Option<String>,
        /// Substring match against operationId and path.
        #[arg(long)]
        grep: Option<String>,
        /// Emit JSON instead of an aligned table.
        #[arg(long)]
        json: bool,
    },
    /// Add a selector to `[server].operations` in the TOML config.
    /// Does not regenerate unless `--regenerate` is passed.
    Add {
        /// Selector: `operationId` | `METHOD /path` | `tag:<name>`.
        selector: Option<String>,
        /// Path to the OpenAPI spec. Defaults to the one in the config.
        #[arg(long)]
        spec: Option<PathBuf>,
        /// Path to the TOML config to edit.
        #[arg(long, default_value = "openapi-to-rust.toml")]
        config: PathBuf,
        /// Expand a tag and add each operationId individually
        /// (instead of adding a `tag:` selector).
        #[arg(long)]
        all_tag: Option<String>,
        /// Print the proposed change without writing.
        #[arg(long)]
        dry_run: bool,
        /// After updating the TOML, immediately run `generate` to
        /// emit code for the new selectors.
        #[arg(long)]
        regenerate: bool,
    },
    /// Remove a selector entry from `[server].operations`.
    Remove {
        /// Selector to remove. Matched verbatim against the TOML list.
        selector: String,
        /// Path to the TOML config to edit.
        #[arg(long, default_value = "openapi-to-rust.toml")]
        config: PathBuf,
        /// Print the proposed change without writing.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let machine_readable_error = matches!(
        &cli.command,
        Commands::Generate { json: true, .. } | Commands::Init { json: true, .. }
    );
    if let Err(e) = run(cli) {
        if machine_readable_error {
            println!(
                "{}",
                serde_json::json!({ "status": "error", "error": e.to_string() })
            );
        } else {
            // Use Display, not Debug, so thiserror messages render with
            // fuzzy-match suggestions instead of raw enum debug output.
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
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
        Commands::Generate {
            source,
            config,
            output_dir,
            module_name,
            types_only,
            types_conservative,
            dry_run,
            check,
            quiet,
            json,
        } => run_generate(GenerateArgs {
            source,
            config,
            output_dir,
            module_name,
            types_only,
            types_conservative,
            dry_run,
            check,
            quiet,
            json,
        }),
        Commands::Init {
            source,
            config,
            output_dir,
            module_name,
            types_only,
            force,
            dry_run,
            quiet,
            json,
        } => run_init(InitArgs {
            source,
            config,
            output_dir,
            module_name,
            types_only,
            force,
            dry_run,
            quiet,
            json,
        }),
        Commands::Server { action } => match action {
            ServerCommands::List {
                spec,
                config,
                tag,
                method,
                grep,
                json,
            } => run_server_list(spec, config, tag, method, grep, json),
            ServerCommands::Add {
                selector,
                spec,
                config,
                all_tag,
                dry_run,
                regenerate,
            } => run_server_add(selector, spec, config, all_tag, dry_run, regenerate),
            ServerCommands::Remove {
                selector,
                config,
                dry_run,
            } => run_server_remove(selector, config, dry_run),
        },
    }
}

struct GenerateArgs {
    source: Option<String>,
    config: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    module_name: Option<String>,
    types_only: bool,
    types_conservative: bool,
    dry_run: bool,
    check: bool,
    quiet: bool,
    json: bool,
}

struct InitArgs {
    source: String,
    config: PathBuf,
    output_dir: PathBuf,
    module_name: Option<String>,
    types_only: bool,
    force: bool,
    dry_run: bool,
    quiet: bool,
    json: bool,
}

#[derive(Serialize)]
struct GenerationSummary {
    status: &'static str,
    source: String,
    output_dir: String,
    schemas: usize,
    operations: usize,
    pruned_schemas: usize,
    files: Vec<String>,
    dependencies: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

fn run_generate(args: GenerateArgs) -> Result<(), Box<dyn std::error::Error>> {
    let (mut generator_config, load_source, provenance) = match args.source {
        Some(source) => {
            let config = GeneratorConfig {
                spec_path: PathBuf::from(&source),
                output_dir: args
                    .output_dir
                    .unwrap_or_else(|| PathBuf::from("src/generated")),
                module_name: args.module_name.unwrap_or_else(|| "api".to_string()),
                enable_async_client: !args.types_only,
                enable_sse_client: false,
                tracing_enabled: false,
                ..Default::default()
            };
            let provenance = sanitize_source_provenance(&source);
            (config, source, provenance)
        }
        None => {
            let config_path = args
                .config
                .unwrap_or_else(|| PathBuf::from("openapi-to-rust.toml"));
            let raw_source = raw_config_spec_source(&config_path)?;
            let config = ConfigFile::load(&config_path)?.into_generator_config();
            let load_source = config.spec_path.to_string_lossy().to_string();
            (config, load_source, sanitize_source_provenance(&raw_source))
        }
    };
    if args.types_conservative {
        generator_config.types = openapi_to_rust::TypeMappingConfig::conservative();
    }

    let spec_content = load_spec(&load_source)?;
    let spec_value = parse_spec(&spec_content, &load_source)?;
    let warning = openapi_to_rust::spec_source::validate_oas_document(&spec_value)?;
    let mapper = openapi_to_rust::TypeMapper::new(generator_config.types.clone());
    let mut analyzer = if generator_config.schema_extensions.is_empty() {
        SchemaAnalyzer::with_type_mapper(spec_value, mapper)?
    } else {
        SchemaAnalyzer::new_with_extensions_and_type_mapper(
            spec_value,
            &generator_config.schema_extensions,
            mapper,
        )?
    };
    let mut analysis = analyzer.analyze()?;
    let generator = CodeGenerator::new(generator_config).with_source_provenance(provenance.clone());
    let result = generator.generate_all(&mut analysis)?;
    let artifacts = generator.output_artifacts(&result);

    let status = if args.check {
        check_artifacts(generator.config().output_dir.as_path(), &artifacts)?;
        "up-to-date"
    } else if args.dry_run {
        "dry-run"
    } else {
        write_artifacts(generator.config().output_dir.as_path(), &artifacts)?;
        "generated"
    };
    let summary = GenerationSummary {
        status,
        source: provenance,
        output_dir: generator.config().output_dir.display().to_string(),
        schemas: analysis.schemas.len(),
        operations: analysis.operations.len(),
        pruned_schemas: result.pruned_schemas,
        files: artifacts
            .keys()
            .map(|path| path.display().to_string())
            .collect(),
        dependencies: result
            .required_deps
            .iter()
            .map(|dependency| dependency.crate_name)
            .collect(),
        warning,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else if !args.quiet {
        if let Some(warning) = &summary.warning {
            eprintln!("Warning: {warning}");
        }
        println!(
            "{} {} file(s) for {} in {}",
            match status {
                "generated" => "Generated",
                "dry-run" => "Would generate",
                _ => "Verified",
            },
            summary.files.len(),
            summary.source,
            summary.output_dir
        );
        if status == "generated" && !result.required_deps.is_empty() {
            eprintln!(
                "Dependency fragment: {}/REQUIRED_DEPS.toml",
                summary.output_dir
            );
        }
        if status == "generated"
            && !generator.config().registry_only
            && let Some(server) = generator.config().server.as_ref()
            && !server.operations.is_empty()
        {
            print_server_hint(&analysis, server);
        }
    }
    Ok(())
}

fn raw_config_spec_source(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let value: toml::Value = toml::from_str(&content)?;
    value
        .get("generator")
        .and_then(|generator| generator.get("spec_path"))
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "configuration is missing generator.spec_path".into())
}

fn write_artifacts(
    output_dir: &std::path::Path,
    artifacts: &std::collections::BTreeMap<PathBuf, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    for (relative, content) in artifacts {
        let path = output_dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
    }
    Ok(())
}

fn check_artifacts(
    output_dir: &std::path::Path,
    artifacts: &std::collections::BTreeMap<PathBuf, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut stale = Vec::new();
    for (relative, expected) in artifacts {
        match std::fs::read_to_string(output_dir.join(relative)) {
            Ok(actual) if actual == *expected => {}
            Ok(_) => stale.push(format!("changed: {}", relative.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                stale.push(format!("missing: {}", relative.display()));
            }
            Err(error) => return Err(error.into()),
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "generated output is stale:\n  {}\nRun generation again to update it.",
            stale.join("\n  ")
        )
        .into())
    }
}

#[derive(Serialize)]
struct StarterConfig {
    generator: StarterGenerator,
    features: StarterFeatures,
    http_client: StarterHttpClient,
}

#[derive(Serialize)]
struct StarterGenerator {
    spec_path: String,
    output_dir: PathBuf,
    module_name: String,
}

#[derive(Serialize)]
struct StarterFeatures {
    enable_async_client: bool,
}

#[derive(Serialize)]
struct StarterHttpClient {
    tracing: StarterTracing,
}

#[derive(Serialize)]
struct StarterTracing {
    enabled: bool,
}

#[derive(Serialize)]
struct InitSummary {
    status: &'static str,
    config: String,
    source: String,
    client: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

fn run_init(args: InitArgs) -> Result<(), Box<dyn std::error::Error>> {
    if args.config.exists() && !args.force {
        return Err(format!(
            "refusing to overwrite {}; pass --force to replace it",
            args.config.display()
        )
        .into());
    }
    let content = load_spec(&args.source)?;
    let value = parse_spec(&content, &args.source)?;
    let warning = openapi_to_rust::spec_source::validate_oas_document(&value)?;

    let spec_path = starter_spec_source(&args.source, &args.config)?;
    let starter = StarterConfig {
        generator: StarterGenerator {
            spec_path,
            output_dir: args.output_dir,
            module_name: args
                .module_name
                .unwrap_or_else(|| starter_module_name(&args.source)),
        },
        features: StarterFeatures {
            enable_async_client: !args.types_only,
        },
        http_client: StarterHttpClient {
            tracing: StarterTracing { enabled: false },
        },
    };
    let rendered = format!(
        "# Created by openapi-to-rust init.\n{}",
        toml::to_string_pretty(&starter)?
    );
    if !args.dry_run {
        if let Some(parent) = args
            .config
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&args.config, &rendered)?;
    }
    let status = if args.dry_run { "dry-run" } else { "created" };
    let summary = InitSummary {
        status,
        config: args.config.display().to_string(),
        source: sanitize_source_provenance(&args.source),
        client: !args.types_only,
        content: args.dry_run.then_some(rendered.clone()),
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else if !args.quiet {
        if let Some(warning) = warning {
            eprintln!("Warning: {warning}");
        }
        if args.dry_run {
            print!("{rendered}");
        } else {
            println!("Created {}", args.config.display());
        }
    }
    Ok(())
}

fn starter_spec_source(
    source: &str,
    config: &std::path::Path,
) -> Result<String, Box<dyn std::error::Error>> {
    if openapi_to_rust::spec_source::is_remote_spec(source) {
        return Ok(source.to_string());
    }
    let source_path = PathBuf::from(source);
    let config_parent = config
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if source_path.is_relative()
        && config_parent.is_none_or(|parent| parent == std::path::Path::new("."))
    {
        Ok(source.to_string())
    } else {
        Ok(source_path.canonicalize()?.display().to_string())
    }
}

fn starter_module_name(source: &str) -> String {
    let candidate = reqwest::Url::parse(source)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(Iterator::last)
                .map(str::to_string)
        })
        .or_else(|| {
            std::path::Path::new(source)
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "api".to_string());
    let stem = candidate
        .strip_suffix(".yaml")
        .or_else(|| candidate.strip_suffix(".yml"))
        .or_else(|| candidate.strip_suffix(".json"))
        .unwrap_or(&candidate);
    let normalized = stem
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if normalized.is_empty() {
        "api".to_string()
    } else {
        normalized
    }
}

fn resolve_server_spec(
    spec: Option<PathBuf>,
    config: &std::path::Path,
) -> Result<(PathBuf, Vec<PathBuf>), Box<dyn std::error::Error>> {
    match spec {
        // An explicit spec is intentionally analyzed as-is. Schema extensions
        // belong to config mode and must not affect a caller that bypasses it.
        Some(path) => Ok((path, Vec::new())),
        None => {
            let cf = ConfigFile::load(config).map_err(|e| {
                format!(
                    "no --spec provided and failed to load {}: {}",
                    config.display(),
                    e
                )
            })?;
            let generator = cf.into_generator_config();
            Ok((generator.spec_path, generator.schema_extensions))
        }
    }
}

fn load_analysis(
    spec_path: &std::path::Path,
    schema_extensions: &[PathBuf],
) -> Result<openapi_to_rust::SchemaAnalysis, Box<dyn std::error::Error>> {
    let source = spec_path.to_string_lossy();
    let spec_content = load_spec(&source)?;
    let spec_value = parse_spec(&spec_content, &source)?;
    let mut analyzer = if schema_extensions.is_empty() {
        SchemaAnalyzer::new(spec_value)?
    } else {
        SchemaAnalyzer::new_with_extensions(spec_value, schema_extensions)?
    };
    Ok(analyzer.analyze()?)
}

fn run_server_list(
    spec: Option<PathBuf>,
    config: PathBuf,
    tag: Option<String>,
    method: Option<String>,
    grep: Option<String>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (spec_path, schema_extensions) = resolve_server_spec(spec, &config)?;
    let analysis = load_analysis(&spec_path, &schema_extensions)?;
    let index = OperationIndex::from_analysis(&analysis);

    let filter = ListFilter { tag, method, grep };
    let output = if json {
        ListOutput::Json
    } else {
        ListOutput::Table
    };
    let (body, _count) = render_list(&index, &filter, output);
    print!("{body}");
    Ok(())
}

fn run_server_add(
    selector: Option<String>,
    spec: Option<PathBuf>,
    config: PathBuf,
    all_tag: Option<String>,
    dry_run: bool,
    regenerate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (spec_path, schema_extensions) = resolve_server_spec(spec, &config)?;
    let analysis = load_analysis(&spec_path, &schema_extensions)?;
    let index = OperationIndex::from_analysis(&analysis);

    // Determine which selectors to add. --all-tag expands; otherwise
    // the single positional selector is added verbatim.
    let to_add: Vec<String> = match (&selector, &all_tag) {
        (Some(_), Some(_)) => {
            return Err("provide either <selector> or --all-tag, not both".into());
        }
        (None, None) => {
            return Err("missing argument: provide <selector> or --all-tag <name>".into());
        }
        (Some(s), None) => vec![s.clone()],
        (None, Some(tag)) => {
            let sel = Selector::Tag(tag.clone());
            let res = resolve_selectors(&[sel], &index)?;
            res.operations
                .iter()
                .map(|op| op.operation_id.clone())
                .collect()
        }
    };

    // Validate every selector resolves before touching the file.
    for s in &to_add {
        let parsed = Selector::parse(s)?;
        let _ = resolve_selectors(&[parsed], &index)?;
    }

    let mut editor = ServerEditor::open(&config)?;
    let mut added: Vec<String> = Vec::new();
    let mut already_present: Vec<String> = Vec::new();
    for s in &to_add {
        if editor.add(s)? {
            added.push(s.clone());
        } else {
            already_present.push(s.clone());
        }
    }

    if dry_run {
        println!("--- dry-run: proposed config ---");
        print!("{}", editor.rendered());
        println!("--- end ---");
    } else {
        editor.save()?;
    }

    // Summary
    for s in &added {
        print_add_summary(s, &analysis, &index)?;
    }
    if !already_present.is_empty() {
        for s in &already_present {
            println!("• `{s}` already in [server].operations — no change.");
        }
    }
    if !dry_run && !added.is_empty() {
        let next_step = if regenerate {
            "Regenerating now..."
        } else {
            "Run `openapi-to-rust generate` to emit code."
        };
        println!(
            "\n✓ Added {} entr{} to {}. {next_step}",
            added.len(),
            if added.len() == 1 { "y" } else { "ies" },
            config.display(),
        );
        if regenerate {
            // Re-exec ourselves in `generate` mode against the same
            // config. We do this via the binary path (current_exe) so
            // we pick up the same compiled artefact the user is using.
            let exe = std::env::current_exe()?;
            let status = std::process::Command::new(exe)
                .arg("generate")
                .arg("--config")
                .arg(&config)
                .status()?;
            if !status.success() {
                return Err(format!("regenerate failed with status {status}").into());
            }
        }
    }
    Ok(())
}

fn print_add_summary(
    selector_str: &str,
    analysis: &openapi_to_rust::SchemaAnalysis,
    index: &OperationIndex,
) -> Result<(), Box<dyn std::error::Error>> {
    let parsed = Selector::parse(selector_str)?;
    let res = resolve_selectors(&[parsed], index)?;
    for op in &res.operations {
        let info = analysis
            .operations
            .get(&op.operation_id)
            .ok_or("operation found in index but missing from analysis")?;
        let tag_part = if op.tags.is_empty() {
            "<untagged>".to_string()
        } else {
            op.tags.join(",")
        };
        println!(
            "\n+ `{}`\n  {} {}  (tag: {})",
            selector_str, op.method, op.path, tag_part
        );
        if let Some(rb) = &info.request_body {
            if let Some(name) = rb.schema_name() {
                println!("  Request:  {name}");
            } else {
                println!("  Request:  (non-JSON body)");
            }
        } else {
            println!("  Request:  (none)");
        }
        if !info.response_schemas.is_empty() {
            let mut parts: Vec<String> = info
                .response_schemas
                .iter()
                .map(|(code, ty)| format!("{code}={ty}"))
                .collect();
            parts.sort();
            println!("  Response: {}", parts.join("  "));
        }
        println!(
            "  Streaming: {}",
            if op.supports_streaming { "yes" } else { "no" }
        );
    }
    Ok(())
}

/// Surface a paste-ready impl skeleton at the end of `generate`.
/// Reads the picked operations from the analysis to name the trait,
/// method, and body type concretely. Goes to stderr so it doesn't
/// pollute machine-readable stdout consumers.
fn print_server_hint(
    analysis: &openapi_to_rust::SchemaAnalysis,
    server: &openapi_to_rust::config::ServerSection,
) {
    use heck::{ToPascalCase, ToSnakeCase};

    // Pick the first resolved op to ground the skeleton in concrete
    // names. Showing one is enough — users extrapolate to siblings.
    let first_op_id = server.operations.first().and_then(|raw| {
        Selector::parse(raw).ok().and_then(|sel| match sel {
            Selector::OperationId(id) => Some(id),
            Selector::MethodPath { method, path } => analysis
                .operations
                .values()
                .find(|op| op.method == method && op.path == path)
                .map(|op| op.operation_id.clone()),
            Selector::Tag(t) => analysis
                .operations
                .values()
                .find(|op| op.tags.iter().any(|tag| tag == &t))
                .map(|op| op.operation_id.clone()),
        })
    });

    let Some(first_op_id) = first_op_id else {
        return;
    };
    let Some(op) = analysis.operations.get(&first_op_id) else {
        return;
    };
    let method = op.operation_id.to_snake_case();
    let response_ty = format!("{}Response", op.operation_id.to_pascal_case());
    let body_param = match &op.request_body {
        Some(rb) => match rb.schema_name() {
            Some(name) => format!(", body: {name}"),
            None => String::new(),
        },
        None => String::new(),
    };
    let tag = op.tags.first().cloned().unwrap_or_else(|| "Server".into());
    let trait_name = format!("{}Api", tag.to_pascal_case());
    let router_fn = format!("{}_router", trait_name.to_snake_case());

    eprintln!();
    eprintln!("📝 Next step — implement the trait:");
    eprintln!();
    eprintln!("   #[derive(Clone)]");
    eprintln!("   pub struct AppState {{ /* state goes here */ }}");
    eprintln!();
    eprintln!("   #[async_trait::async_trait]");
    eprintln!("   impl {trait_name} for AppState {{");
    eprintln!("       async fn {method}(&self{body_param}) -> {response_ty} {{");
    eprintln!("           todo!()");
    eprintln!("       }}");
    eprintln!("   }}");
    eprintln!();
    eprintln!("   // In main():");
    eprintln!("   let app = {router_fn}(AppState {{ /* … */ }});");
    eprintln!();
    if op.supports_streaming {
        let (stream_variant, runtime_status) = analysis
            .operation_responses
            .get(&op.operation_id)
            .and_then(|responses| {
                responses
                    .iter()
                    .find(|(_, response)| response.supports_streaming)
            })
            .map(|(status, _)| {
                (
                    format!(
                        "{}Stream",
                        openapi_to_rust::server::codegen::status_variant_name(status)
                    ),
                    status == "default" || status.ends_with("XX"),
                )
            })
            .unwrap_or_else(|| ("OkStream".to_string(), false));
        let stream_args = if runtime_status {
            "status_code, sse_response(your_stream)"
        } else {
            "sse_response(your_stream)"
        };
        eprintln!("   For streaming, return `{response_ty}::{stream_variant}({stream_args})`.");
    }
}

fn run_server_remove(
    selector: String,
    config: PathBuf,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut editor = ServerEditor::open(&config)?;
    let removed = editor.remove(&selector)?;
    if !removed {
        println!("• `{selector}` not present in [server].operations — no change.");
        return Ok(());
    }
    if dry_run {
        println!("--- dry-run: proposed config ---");
        print!("{}", editor.rendered());
        println!("--- end ---");
    } else {
        editor.save()?;
        println!(
            "✓ Removed `{selector}` from {}. Handler code in your crate may now be dead — review.",
            config.display()
        );
    }
    Ok(())
}
