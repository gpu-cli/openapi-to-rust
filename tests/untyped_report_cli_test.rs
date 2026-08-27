//! `--report-untyped` at the CLI boundary.
//!
//! The census is only useful if someone can run it on their own spec and act on
//! the answer, so these tests pin the surface: the human-readable summary names
//! the reason and the field, and `--json` emits findings a script can rank.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

/// One field the generator can type, one it cannot, and one the schema left
/// open — so the report has something to say in each column.
const SPEC: &str = r#"openapi: 3.1.0
info:
  title: untyped report
  version: 1.0.0
components:
  schemas:
    Thing:
      type: object
      additionalProperties: false
      properties:
        name:
          type: string
        metadata:
          type: object
"#;

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_openapi-to-rust"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .current_dir(dir)
        .args(args)
        .output()
        .expect("cli runs")
}

fn write_spec(dir: &Path) {
    std::fs::write(dir.join("api.yaml"), SPEC).expect("spec written");
}

#[test]
fn report_untyped_names_the_reason_and_the_field() {
    let dir = TempDir::new().expect("tempdir");
    write_spec(dir.path());

    let output = run(
        dir.path(),
        &[
            "generate",
            "api.yaml",
            "--output-dir",
            "out",
            "--types-only",
            "--report-untyped",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("untyped: 1 field(s)"),
        "expected a count, got:\n{stdout}"
    );
    assert!(
        stdout.contains("OpaqueObject") && stdout.contains("Thing.metadata"),
        "expected the reason and the field, got:\n{stdout}"
    );
    assert!(
        stdout.contains("faithful"),
        "an unconstrained object is faithful, not a defect:\n{stdout}"
    );
}

#[test]
fn report_untyped_json_emits_findings_for_tooling() {
    let dir = TempDir::new().expect("tempdir");
    write_spec(dir.path());

    let output = run(
        dir.path(),
        &[
            "generate",
            "api.yaml",
            "--output-dir",
            "out",
            "--types-only",
            "--report-untyped",
            "--json",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The findings array is printed before the generation summary; a consumer
    // reads the first JSON value, which is what scripts/untyped-census.sh does.
    let findings: serde_json::Value = serde_json::Deserializer::from_str(&stdout)
        .into_iter()
        .next()
        .expect("a JSON value on stdout")
        .expect("valid JSON");
    let findings = findings.as_array().expect("findings are an array");

    assert_eq!(findings.len(), 1, "{findings:?}");
    assert_eq!(findings[0]["context"], "Thing.metadata");
    assert_eq!(findings[0]["reason"], "opaque-object");
    assert_eq!(findings[0]["shape"], "value");
}

#[test]
fn a_fully_typed_spec_reports_none() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(
        dir.path().join("api.yaml"),
        SPEC.replace("        metadata:\n          type: object\n", ""),
    )
    .expect("spec written");

    let output = run(
        dir.path(),
        &[
            "generate",
            "api.yaml",
            "--output-dir",
            "out",
            "--types-only",
            "--report-untyped",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("untyped: none"),
        "expected the clean-bill message, got:\n{stdout}"
    );
}
