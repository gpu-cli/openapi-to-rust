//! Manifest-driven generation and verification for collections of API clients.
//!
//! The same implementation backs local `openapi-to-rust clients` commands and
//! the GitHub Action. GitHub-specific concerns such as branches and pull
//! requests intentionally stay outside this module.

use crate::cli::load_spec;
use crate::spec_source::{parse_spec, sanitize_source_provenance, validate_oas_document};
use crate::{CodeGenerator, ConfigFile, SchemaAnalyzer, TypeMapper};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClientMode {
    Check,
    Update,
    Sync,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientManifest {
    pub version: u32,
    pub clients: Vec<ClientEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientEntry {
    pub name: String,
    pub spec: SpecSource,
    pub config: PathBuf,
    pub cargo: CargoCheck,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SpecSource {
    Local(LocalSpec),
    Remote(RemoteSpec),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalSpec {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteSpec {
    pub url: String,
    pub vendor: PathBuf,
    #[serde(default)]
    pub normalize: Normalize,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Normalize {
    /// JSON pointers removed before comparing the fetched and vendored specs.
    pub strip: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CargoCheck {
    pub manifest_path: PathBuf,
    pub package: Option<String>,
    pub all_features: bool,
    pub locked: bool,
    pub test: bool,
}

impl Default for CargoCheck {
    fn default() -> Self {
        Self {
            manifest_path: PathBuf::from("Cargo.toml"),
            package: None,
            all_features: true,
            locked: true,
            test: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientRunReport {
    pub mode: ClientMode,
    pub manifest: String,
    pub clients: Vec<ClientResult>,
}

impl ClientRunReport {
    pub fn failed(&self) -> usize {
        self.clients
            .iter()
            .filter(|client| client.error.is_some())
            .count()
    }

    pub fn changed(&self) -> usize {
        self.clients
            .iter()
            .filter(|client| client.spec_changed || client.output_changed)
            .count()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientResult {
    pub name: String,
    pub spec_changed: bool,
    pub output_changed: bool,
    pub generated: bool,
    pub compiled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug)]
struct ResolvedEntry {
    entry: ClientEntry,
    config: PathBuf,
    spec_path: PathBuf,
    cargo_manifest: PathBuf,
}

pub fn run_clients(
    manifest_path: &Path,
    mode: ClientMode,
    selected: Option<&str>,
) -> Result<ClientRunReport, Box<dyn std::error::Error>> {
    let manifest_path = manifest_path.canonicalize().map_err(|error| {
        format!(
            "failed to resolve client manifest '{}': {error}",
            manifest_path.display()
        )
    })?;
    let root = manifest_path
        .parent()
        .ok_or_else(|| "client manifest has no parent directory".to_string())?;
    let manifest = load_manifest(&manifest_path)?;
    let entries = resolve_entries(manifest, root, selected)?;
    let mut results = Vec::with_capacity(entries.len());

    for resolved in entries {
        let mut result = ClientResult {
            name: resolved.entry.name.clone(),
            spec_changed: false,
            output_changed: false,
            generated: false,
            compiled: false,
            error: None,
        };
        if let Err(error) = run_one(&resolved, mode, &mut result) {
            result.error = Some(error.to_string());
        }
        results.push(result);
    }

    Ok(ClientRunReport {
        mode,
        manifest: manifest_path.display().to_string(),
        clients: results,
    })
}

fn load_manifest(path: &Path) -> Result<ClientManifest, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path)?;
    let manifest: ClientManifest = serde_yaml::from_str(&raw)
        .map_err(|error| format!("failed to parse '{}': {error}", path.display()))?;
    if manifest.version != MANIFEST_VERSION {
        return Err(format!(
            "unsupported client manifest version {}; expected {}",
            manifest.version, MANIFEST_VERSION
        )
        .into());
    }
    if manifest.clients.is_empty() {
        return Err("client manifest must declare at least one client".into());
    }
    let mut names = BTreeSet::new();
    for client in &manifest.clients {
        if client.name.trim().is_empty() {
            return Err("client names cannot be empty".into());
        }
        if !names.insert(client.name.as_str()) {
            return Err(format!("duplicate client name '{}'", client.name).into());
        }
        if let SpecSource::Remote(remote) = &client.spec {
            crate::spec_source::validate_remote_spec_url(&remote.url)
                .map_err(|error| format!("client '{}': {error}", client.name))?;
            for pointer in &remote.normalize.strip {
                if !pointer.is_empty() && !pointer.starts_with('/') {
                    return Err(format!(
                        "client '{}': normalize.strip value '{pointer}' must be a JSON pointer",
                        client.name
                    )
                    .into());
                }
            }
        }
    }
    Ok(manifest)
}

fn resolve_entries(
    manifest: ClientManifest,
    root: &Path,
    selected: Option<&str>,
) -> Result<Vec<ResolvedEntry>, Box<dyn std::error::Error>> {
    let mut found = false;
    let mut entries = Vec::new();
    for entry in manifest.clients {
        if selected.is_some_and(|name| name != entry.name) {
            continue;
        }
        found = true;
        let spec_path = match &entry.spec {
            SpecSource::Local(local) => resolve_path(root, &local.path),
            SpecSource::Remote(remote) => resolve_path(root, &remote.vendor),
        };
        entries.push(ResolvedEntry {
            config: resolve_path(root, &entry.config),
            cargo_manifest: resolve_path(root, &entry.cargo.manifest_path),
            spec_path,
            entry,
        });
    }
    if !found {
        return Err(format!(
            "client '{}' is not declared in the manifest",
            selected.unwrap_or_default()
        )
        .into());
    }
    Ok(entries)
}

fn resolve_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn run_one(
    resolved: &ResolvedEntry,
    mode: ClientMode,
    result: &mut ClientResult,
) -> Result<(), Box<dyn std::error::Error>> {
    if mode == ClientMode::Sync
        && let SpecSource::Remote(remote) = &resolved.entry.spec
    {
        result.spec_changed = sync_remote_spec(remote, &resolved.spec_path)?;
    }

    if !resolved.spec_path.exists() {
        let hint = match resolved.entry.spec {
            SpecSource::Remote(_) => "run `openapi-to-rust clients sync` first",
            SpecSource::Local(_) => "check the manifest's spec.path",
        };
        return Err(format!(
            "specification '{}' does not exist; {hint}",
            resolved.spec_path.display()
        )
        .into());
    }

    result.output_changed = generate_from_config(
        &resolved.config,
        &resolved.spec_path,
        mode == ClientMode::Check,
    )?;
    result.generated = true;
    run_cargo(&resolved.entry.cargo, &resolved.cargo_manifest)?;
    result.compiled = true;
    Ok(())
}

fn sync_remote_spec(
    remote: &RemoteSpec,
    vendor_path: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let incoming = load_spec(&remote.url)?;
    update_vendored_spec(&incoming, &remote.url, vendor_path, &remote.normalize.strip)
}

fn update_vendored_spec(
    incoming: &str,
    incoming_source: &str,
    vendor_path: &Path,
    strip: &[String],
) -> Result<bool, Box<dyn std::error::Error>> {
    let incoming_normalized = normalize_spec(incoming, incoming_source, strip)?;
    let existing_normalized = std::fs::read_to_string(vendor_path)
        .ok()
        .and_then(|existing| {
            normalize_spec(&existing, &vendor_path.display().to_string(), strip).ok()
        });
    if existing_normalized.as_deref() == Some(incoming_normalized.as_slice()) {
        return Ok(false);
    }
    if let Some(parent) = vendor_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(vendor_path, incoming)?;
    Ok(true)
}

fn normalize_spec(
    raw: &str,
    source: &str,
    strip: &[String],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut value = parse_spec(raw, source)?;
    validate_oas_document(&value)?;
    for pointer in strip {
        strip_pointer(&mut value, pointer);
    }
    Ok(serde_json::to_vec(&value)?)
}

fn strip_pointer(value: &mut serde_json::Value, pointer: &str) {
    let Some((parent, encoded_key)) = pointer.rsplit_once('/') else {
        return;
    };
    let key = encoded_key.replace("~1", "/").replace("~0", "~");
    let target = if parent.is_empty() {
        Some(value)
    } else {
        value.pointer_mut(parent)
    };
    match target {
        Some(serde_json::Value::Object(map)) => {
            map.remove(&key);
        }
        Some(serde_json::Value::Array(items)) => {
            if let Ok(index) = key.parse::<usize>()
                && index < items.len()
            {
                items.remove(index);
            }
        }
        _ => {}
    }
}

fn generate_from_config(
    config_path: &Path,
    expected_spec: &Path,
    check: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let provenance = raw_config_spec_source(config_path)?;
    let mut generator_config = ConfigFile::load(config_path)?.into_generator_config();
    let configured_spec = generator_config.spec_path.canonicalize()?;
    let expected_spec = expected_spec.canonicalize()?;
    if configured_spec != expected_spec {
        return Err(format!(
            "config '{}' uses spec '{}', but the client manifest declares '{}'",
            config_path.display(),
            configured_spec.display(),
            expected_spec.display()
        )
        .into());
    }

    let source = generator_config.spec_path.to_string_lossy().to_string();
    let raw = load_spec(&source)?;
    let value = parse_spec(&raw, &source)?;
    validate_oas_document(&value)?;
    generator_config.apply_spec_server_default(&value);
    let mapper = TypeMapper::new(generator_config.types.clone());
    let mut analyzer = if generator_config.schema_extensions.is_empty() {
        SchemaAnalyzer::with_type_mapper(value, mapper)?
    } else {
        SchemaAnalyzer::new_with_extensions_and_type_mapper(
            value,
            &generator_config.schema_extensions,
            mapper,
        )?
    };
    let mut analysis = analyzer.analyze()?;
    let generator = CodeGenerator::new(generator_config)
        .with_source_provenance(sanitize_source_provenance(&provenance));
    let generated = generator.generate_all(&mut analysis)?;
    let artifacts = generator.output_artifacts(&generated);
    let changed = artifacts_changed(generator.config().output_dir.as_path(), &artifacts)?;
    if check {
        check_artifacts(generator.config().output_dir.as_path(), &artifacts)?;
    } else {
        write_artifacts(generator.config().output_dir.as_path(), &artifacts)?;
    }
    Ok(changed)
}

fn raw_config_spec_source(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
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
    output_dir: &Path,
    artifacts: &BTreeMap<PathBuf, String>,
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
    output_dir: &Path,
    artifacts: &BTreeMap<PathBuf, String>,
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
            "generated output is stale:\n  {}\nRun `openapi-to-rust clients update` and commit the result.",
            stale.join("\n  ")
        )
        .into())
    }
}

fn artifacts_changed(
    output_dir: &Path,
    artifacts: &BTreeMap<PathBuf, String>,
) -> Result<bool, Box<dyn std::error::Error>> {
    for (relative, expected) in artifacts {
        match std::fs::read_to_string(output_dir.join(relative)) {
            Ok(actual) if actual == *expected => {}
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}

fn run_cargo(cargo: &CargoCheck, manifest_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !manifest_path.exists() {
        return Err(format!(
            "Cargo manifest '{}' does not exist",
            manifest_path.display()
        )
        .into());
    }
    run_cargo_command("check", cargo, manifest_path)?;
    if cargo.test {
        run_cargo_command("test", cargo, manifest_path)?;
    }
    Ok(())
}

fn run_cargo_command(
    subcommand: &str,
    cargo: &CargoCheck,
    manifest_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::new("cargo");
    command
        .arg(subcommand)
        .arg("--manifest-path")
        .arg(manifest_path);
    if let Some(package) = &cargo.package {
        command.arg("--package").arg(package);
    }
    if cargo.all_features {
        command.arg("--all-features");
    }
    if cargo.locked {
        command.arg("--locked");
    }
    // Keep the CLI's stdout available for its JSON report. Cargo test harnesses
    // write regular output to stdout, which would otherwise corrupt `--json`
    // when a caller redirects the command's stdout to a report file.
    let output = command.output()?;
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&output.stdout)?;
    stderr.write_all(&output.stderr)?;
    stderr.flush()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "cargo {subcommand} failed for '{}' with {}",
            output.status,
            manifest_path.display()
        )
        .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_duplicate_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clients.yml");
        std::fs::write(
            &path,
            r#"
version: 1
clients:
  - name: sample
    spec: { path: openapi.yaml }
    config: openapi-to-rust.toml
    cargo: {}
  - name: sample
    spec: { path: another.yaml }
    config: another.toml
    cargo: {}
"#,
        )
        .unwrap();
        let error = load_manifest(&path).unwrap_err().to_string();
        assert!(error.contains("duplicate client name 'sample'"));
    }

    #[test]
    fn normalization_ignores_order_and_configured_fields() {
        let left =
            r#"{"openapi":"3.1.0","info":{"title":"A","version":"1","x-built":"one"},"paths":{}}"#;
        let right =
            r#"{"paths":{},"info":{"x-built":"two","version":"1","title":"A"},"openapi":"3.1.0"}"#;
        let strip = vec!["/info/x-built".to_string()];
        assert_eq!(
            normalize_spec(left, "left.json", &strip).unwrap(),
            normalize_spec(right, "right.json", &strip).unwrap()
        );
    }

    #[test]
    fn check_detects_stale_generated_output_and_update_repairs_it() {
        let dir = tempfile::tempdir().unwrap();
        let spec = dir.path().join("openapi.yaml");
        let config = dir.path().join("openapi-to-rust.toml");
        std::fs::write(
            &spec,
            "openapi: 3.1.0\ninfo: { title: Sample, version: '1' }\npaths: {}\ncomponents:\n  schemas:\n    Greeting:\n      type: object\n      properties:\n        message: { type: string }\n",
        )
        .unwrap();
        std::fs::write(
            &config,
            "[generator]\nspec_path = \"openapi.yaml\"\noutput_dir = \"src/generated\"\nmodule_name = \"sample\"\n\n[features]\nenable_async_client = false\n",
        )
        .unwrap();

        let error = generate_from_config(&config, &spec, true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("generated output is stale"));
        assert!(generate_from_config(&config, &spec, false).unwrap());
        assert!(!generate_from_config(&config, &spec, true).unwrap());
    }

    #[test]
    fn vendored_remote_changes_only_when_normalized_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        let vendor = dir.path().join("openapi.json");
        let existing =
            r#"{"openapi":"3.1.0","info":{"title":"A","version":"1","x-built":"one"},"paths":{}}"#;
        let reordered =
            r#"{"paths":{},"info":{"x-built":"two","version":"1","title":"A"},"openapi":"3.1.0"}"#;
        let changed = r#"{"openapi":"3.1.0","info":{"title":"B","version":"1"},"paths":{}}"#;
        let strip = vec!["/info/x-built".to_string()];
        std::fs::write(&vendor, existing).unwrap();

        assert!(!update_vendored_spec(reordered, "remote", &vendor, &strip).unwrap());
        assert_eq!(std::fs::read_to_string(&vendor).unwrap(), existing);
        assert!(update_vendored_spec(changed, "remote", &vendor, &strip).unwrap());
        assert_eq!(std::fs::read_to_string(&vendor).unwrap(), changed);
    }

    #[test]
    fn manifest_update_then_check_generates_and_compiles_a_client() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("crate/src")).unwrap();
        std::fs::write(
            dir.path().join("openapi.yaml"),
            "openapi: 3.1.0\ninfo: { title: Sample, version: '1' }\npaths: {}\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("openapi-to-rust.toml"),
            "[generator]\nspec_path = \"openapi.yaml\"\noutput_dir = \"crate/src/generated\"\nmodule_name = \"sample\"\n\n[features]\nenable_async_client = false\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("crate/Cargo.toml"),
            "[package]\nname = \"sample-client\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("crate/src/lib.rs"), "pub mod generated;\n").unwrap();
        let manifest_path = dir.path().join("openapi-clients.yml");
        std::fs::write(
            &manifest_path,
            "version: 1\nclients:\n  - name: sample\n    spec: { path: openapi.yaml }\n    config: openapi-to-rust.toml\n    cargo:\n      manifest_path: crate/Cargo.toml\n      package: sample-client\n      all_features: false\n      locked: false\n",
        )
        .unwrap();

        let updated = run_clients(&manifest_path, ClientMode::Update, None).unwrap();
        assert_eq!(updated.failed(), 0);
        assert_eq!(updated.changed(), 1);
        let checked = run_clients(&manifest_path, ClientMode::Check, None).unwrap();
        assert_eq!(checked.failed(), 0);
        assert_eq!(checked.changed(), 0);
    }
}
