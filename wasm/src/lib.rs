//! WASM bindings for the openapi-to-rust playground.
//!
//! Wraps the in-memory generation pipeline (`SchemaAnalyzer` →
//! `CodeGenerator::generate_all`) for the browser. No filesystem or network
//! access — the page hands us a spec string, we hand back rendered files.

use openapi_to_rust::spec_source::{
    json_from_str_lossy, sanitize_source_provenance, validate_oas_document, yaml_to_json_value,
};
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Module name used for the generated tree, matching the CLI's spec-argument
/// default (`openapi-to-rust generate <SOURCE>`).
const MODULE_NAME: &str = "api";

#[derive(Serialize)]
struct PlaygroundFile {
    name: String,
    content: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaygroundOutput {
    /// Files as the CLI writes them into the output directory.
    files: Vec<PlaygroundFile>,
    /// A complete, compilable crate: Cargo.toml, src/lib.rs, src/api/*.
    crate_files: Vec<PlaygroundFile>,
    crate_name: String,
    /// Non-fatal warning (e.g. experimental OpenAPI 3.2 support).
    warning: Option<String>,
    schemas: usize,
    operations: usize,
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// Version of the underlying openapi-to-rust generator crate.
#[wasm_bindgen]
pub fn version() -> String {
    openapi_to_rust::VERSION.to_string()
}

/// Generate Rust source files from an OpenAPI document (YAML or JSON).
///
/// `source_label` names where the document came from (a URL, or a playground
/// marker for pasted content) and is recorded in generated file headers the
/// same way the CLI records the spec path.
///
/// Returns `{ files, crateFiles, crateName, warning, schemas, operations }`.
/// Errors are thrown as JS `Error`s carrying the generator's message.
#[wasm_bindgen]
pub fn generate(spec: &str, source_label: &str) -> Result<JsValue, JsError> {
    let trimmed = spec.trim_start();
    if trimmed.is_empty() {
        return Err(JsError::new("the OpenAPI document is empty"));
    }
    // JSON documents start with `{`; everything else goes through the YAML
    // path, which applies the large-integer sanitization the CLI uses.
    let value = if trimmed.starts_with('{') {
        json_from_str_lossy(spec)
    } else {
        yaml_to_json_value(spec)
    }
    .map_err(|error| JsError::new(&format!("failed to parse the document: {error}")))?;

    let warning = validate_oas_document(&value).map_err(|message| JsError::new(&message))?;
    let crate_name = crate_name_from_spec(&value);

    let mut analyzer =
        SchemaAnalyzer::new(value).map_err(|error| JsError::new(&error.to_string()))?;
    let mut analysis = analyzer
        .analyze()
        .map_err(|error| JsError::new(&error.to_string()))?;

    // Mirror the CLI's `generate <SOURCE>` configuration so playground output
    // is byte-identical to a local run (modulo the source label).
    let config = GeneratorConfig {
        module_name: MODULE_NAME.to_string(),
        enable_async_client: true,
        enable_sse_client: false,
        tracing_enabled: false,
        ..GeneratorConfig::default()
    };
    let generator =
        CodeGenerator::new(config).with_source_provenance(sanitize_source_provenance(source_label));
    let result = generator
        .generate_all(&mut analysis)
        .map_err(|error| JsError::new(&error.to_string()))?;

    let files: Vec<PlaygroundFile> = generator
        .output_artifacts(&result)
        .into_iter()
        .map(|(path, content)| PlaygroundFile {
            name: path.display().to_string(),
            content,
        })
        .collect();
    let crate_files = crate_files(&crate_name, &files);

    let output = PlaygroundOutput {
        files,
        crate_files,
        crate_name,
        warning,
        schemas: analysis.schemas.len(),
        operations: analysis.operations.len(),
    };
    serde_wasm_bindgen::to_value(&output).map_err(|error| JsError::new(&error.to_string()))
}

/// Derive a cargo package name from the spec's `info.title`.
fn crate_name_from_spec(value: &serde_json::Value) -> String {
    let title = value
        .get("info")
        .and_then(|info| info.get("title"))
        .and_then(|title| title.as_str())
        .unwrap_or("");
    let mut name = String::new();
    let mut previous_dash = true;
    for character in title.chars() {
        if character.is_ascii_alphanumeric() {
            name.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            name.push('-');
            previous_dash = true;
        }
    }
    let name = name.trim_matches('-');
    if name.is_empty() {
        "generated-api".to_string()
    } else if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("api-{name}")
    } else {
        name.to_string()
    }
}

/// Assemble a complete crate from the CLI-shaped output files: the generated
/// tree mounts at `src/api/`, `src/lib.rs` re-exports it, and Cargo.toml is
/// the REQUIRED_DEPS fragment under a package header.
fn crate_files(crate_name: &str, files: &[PlaygroundFile]) -> Vec<PlaygroundFile> {
    let mut out = Vec::new();
    let mut dependencies_section = String::from("[dependencies]\n");
    for file in files {
        if file.name == "REQUIRED_DEPS.toml" {
            if let Some(index) = file.content.find("[dependencies]") {
                dependencies_section = file.content[index..].to_string();
            }
        } else {
            out.push(PlaygroundFile {
                name: format!("src/{MODULE_NAME}/{}", file.name),
                content: file.content.clone(),
            });
        }
    }
    out.push(PlaygroundFile {
        name: "Cargo.toml".to_string(),
        content: format!(
            "[package]\n\
             name = \"{crate_name}\"\n\
             version = \"0.1.0\"\n\
             edition = \"2024\"\n\
             \n\
             {dependencies_section}"
        ),
    });
    out.push(PlaygroundFile {
        name: "src/lib.rs".to_string(),
        content: format!("pub mod {MODULE_NAME};\n"),
    });
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
