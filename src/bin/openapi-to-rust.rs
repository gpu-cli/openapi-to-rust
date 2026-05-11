use clap::{Parser, Subcommand};
use openapi_to_rust::cli::{json_from_str_lossy, yaml_to_json_value};
use openapi_to_rust::server::{
    OperationIndex, Selector,
    edit::Editor as ServerEditor,
    list::{ListFilter, ListOutput, render as render_list},
    resolve as resolve_selectors,
};
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
        /// Force every typed-scalar strategy back to "string" (Q2).
        /// Useful for bisecting regressions caused by typed-scalar
        /// adoption — overrides any `[generator.types]` settings in
        /// the TOML config.
        #[arg(long)]
        types_conservative: bool,
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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        // Use Display, not Debug, so thiserror messages render with
        // their fuzzy-match suggestions (`Did you mean ...?`) instead
        // of as raw enum debug output.
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
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
            config,
            types_conservative,
        } => {
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

            let mut generator_config = config_file.into_generator_config();

            // CLI override: `--types-conservative` collapses every
            // Q2 typed-scalar strategy back to plain `String`. Useful
            // for bisecting regressions caused by typed-scalar
            // adoption without editing the TOML config.
            if types_conservative {
                generator_config.types = openapi_to_rust::TypeMappingConfig::conservative();
            }

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
            let type_mapper = openapi_to_rust::TypeMapper::new(generator_config.types.clone());
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

            // P4: server-side scaffolding. Runs only when [server] is
            // set in the TOML and selectors resolve cleanly.
            if let Some(server_section) = generator.config().server.as_ref() {
                if !server_section.operations.is_empty() {
                    use openapi_to_rust::server::codegen::ServerCodegen;
                    println!("⚙️  Generating server scaffolding (axum)...");
                    let server_files =
                        ServerCodegen::new(generator.config(), &analysis, server_section)
                            .generate()?;
                    let out = generator.config().output_dir.clone();
                    let server_dir = out.join("server");
                    std::fs::create_dir_all(&server_dir)?;
                    for f in &server_files {
                        let path = out.join(&f.path);
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&path, &f.content)?;
                    }
                    // Append server module declaration to mod.rs.
                    let mod_path = out.join("mod.rs");
                    if mod_path.exists() {
                        let body = std::fs::read_to_string(&mod_path)?;
                        if !body.contains("pub mod server") {
                            let mut updated = body;
                            if !updated.ends_with('\n') {
                                updated.push('\n');
                            }
                            updated.push_str("\npub mod server;\npub use server::*;\n");
                            std::fs::write(&mod_path, updated)?;
                        }
                    }
                    println!(
                        "✅ Wrote {} server files to {}/server/",
                        server_files.len(),
                        out.display()
                    );
                    print_server_hint(&analysis, server_section);
                }
            }

            // Q2.8 dep advisory: surface optional crates the
            // generated code references so the operator knows what
            // to add to their Cargo.toml. write_files already
            // dropped a copy-pasteable REQUIRED_DEPS.toml next to
            // the generated module; the stderr summary makes it
            // discoverable without scanning the output dir.
            if !result.required_deps.is_empty() {
                eprintln!();
                eprintln!(
                    "📦 Generated code uses {} optional crate(s). Add to your Cargo.toml:",
                    result.required_deps.len()
                );
                eprintln!();
                eprintln!("[dependencies]");
                for dep in &result.required_deps {
                    eprintln!("{}", dep.to_toml_line());
                }
                eprintln!();
                eprintln!(
                    "(Same content written to {}/REQUIRED_DEPS.toml)",
                    generator.config().output_dir.display()
                );
            }

            Ok(())
        }
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

fn resolve_spec_path(
    spec: Option<PathBuf>,
    config: &std::path::Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    match spec {
        Some(p) => Ok(p),
        None => {
            let cf = ConfigFile::load(config).map_err(|e| {
                format!(
                    "no --spec provided and failed to load {}: {}",
                    config.display(),
                    e
                )
            })?;
            Ok(cf.into_generator_config().spec_path)
        }
    }
}

fn load_analysis(
    spec_path: &std::path::Path,
) -> Result<openapi_to_rust::SchemaAnalysis, Box<dyn std::error::Error>> {
    let spec_content = std::fs::read_to_string(spec_path)?;
    let spec_value: serde_json::Value = if spec_path.extension()
        == Some(std::ffi::OsStr::new("yaml"))
        || spec_path.extension() == Some(std::ffi::OsStr::new("yml"))
    {
        yaml_to_json_value(&spec_content)?
    } else {
        json_from_str_lossy(&spec_content)?
    };
    let mut analyzer = SchemaAnalyzer::new(spec_value)?;
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
    let spec_path = resolve_spec_path(spec, &config)?;
    let analysis = load_analysis(&spec_path)?;
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
    let spec_path = resolve_spec_path(spec, &config)?;
    let analysis = load_analysis(&spec_path)?;
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
    eprintln!("   #[axum::async_trait]");
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
        eprintln!("   For streaming, return `{response_ty}::OkStream(sse_response(your_stream))`.");
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
