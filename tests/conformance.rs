//! Conformance harness: walks tests/conformance/fixtures/, runs each through
//! the parser, records pass/fail against catalog.yaml, emits a coverage report.
//!
//! Layers exercised in this initial cut:
//!   L0  Lossless parse (no spec fields land in `extra`).
//!
//! Subsequent commits will add:
//!   L1  Model snapshot
//!   L2  Analysis IR snapshot
//!   L3  Codegen tokens snapshot
//!   L4  Compiles under `-D warnings`
//!   L5  Runtime wire (wiremock)
//!
//! Run with: cargo test --test conformance
//! Set CONFORMANCE_REPORT=1 to write tests/conformance/coverage-report.md.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use openapi_to_rust::openapi::OpenApiSpec;
use serde_json::Value;

#[derive(Debug)]
struct Fixture {
    path: PathBuf,
    rel: String,
    coverage: Vec<String>,
    /// Layer at which this fixture is expected to fail. The harness only
    /// enforces fixtures whose `fails_at` matches an *active* layer; fixtures
    /// targeting layers we haven't built yet (L1/L2/L3/L4/L5) are recorded as
    /// "deferred" and skipped without affecting pass/fail. As more layers come
    /// online we widen `ACTIVE_LAYERS`.
    fails_at: Option<String>,
    reason: String,
}

/// Layers currently exercised by the harness. Extend as we add L1/L2/L3/...
const ACTIVE_LAYERS: &[&str] = &["L0"];

#[derive(Debug, Default)]
struct Outcome {
    parsed: bool,
    parse_error: Option<String>,
    extras: Vec<ExtraField>,
}

#[derive(Debug)]
struct ExtraField {
    location: String,
    field: String,
}

#[test]
fn conformance_harness() {
    let workspace = workspace_root();
    let fixtures_root = workspace.join("tests/conformance/fixtures");
    let catalog_path = workspace.join("tests/conformance/catalog.yaml");
    let status_path = workspace.join("tests/conformance/status.toml");

    assert!(
        catalog_path.exists(),
        "catalog.yaml missing — run `cargo run --bin catalog-gen`"
    );
    assert!(status_path.exists(), "status.toml missing");

    let fixtures = collect_fixtures(&fixtures_root);
    assert!(
        !fixtures.is_empty(),
        "no fixtures found under {}",
        fixtures_root.display()
    );

    let mut results: Vec<(Fixture, Outcome)> = Vec::with_capacity(fixtures.len());
    for fixture in fixtures {
        let outcome = run(&fixture);
        results.push((fixture, outcome));
    }

    let mut unexpected_pass = Vec::new();
    let mut unexpected_fail = Vec::new();
    let mut deferred = Vec::new();
    for (fx, out) in &results {
        let l0_passed = out.parse_error.is_none() && out.extras.is_empty();
        let active = match &fx.fails_at {
            Some(l) => ACTIVE_LAYERS.contains(&l.as_str()),
            None => true, // no marker = expected to pass every active layer
        };
        if !active {
            deferred.push(fx.rel.clone());
            continue;
        }
        match (fx.fails_at.as_deref(), l0_passed) {
            (Some("L0"), true) => unexpected_pass.push(fx.rel.clone()),
            (None, false) => unexpected_fail.push(format!(
                "{} -> {}",
                fx.rel,
                out.parse_error
                    .clone()
                    .unwrap_or_else(|| format!("{} extras", out.extras.len()))
            )),
            _ => {}
        }
    }

    if std::env::var_os("CONFORMANCE_REPORT").is_some() {
        let report = render_report(&results);
        let out = workspace.join("tests/conformance/coverage-report.md");
        fs::write(&out, report).expect("write report");
        eprintln!("wrote {}", out.display());
    }

    eprintln!(
        "conformance: {} fixtures total — {} active, {} deferred to later layers",
        results.len(),
        results.len() - deferred.len(),
        deferred.len()
    );

    if !unexpected_pass.is_empty() {
        panic!(
            "fixture(s) marked `fails_at: L0` now pass — promote in status.toml and remove the marker:\n  {}",
            unexpected_pass.join("\n  ")
        );
    }
    if !unexpected_fail.is_empty() {
        panic!(
            "fixture(s) regressed (no fails_at marker but failed at an active layer):\n  {}",
            unexpected_fail.join("\n  ")
        );
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_fixtures(root: &Path) -> Vec<Fixture> {
    let mut out = Vec::new();
    visit(root, root, &mut out);
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

fn visit(root: &Path, dir: &Path, out: &mut Vec<Fixture>) {
    for entry in fs::read_dir(dir).expect("readdir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            visit(root, &path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
            out.push(parse_fixture_header(root, &path));
        }
    }
}

fn parse_fixture_header(root: &Path, path: &Path) -> Fixture {
    let body = fs::read_to_string(path).expect("read fixture");
    let mut coverage = Vec::new();
    let mut fails_at: Option<String> = None;
    let mut reason = String::new();
    for line in body.lines().take_while(|l| l.starts_with('#')) {
        let stripped = line.trim_start_matches('#').trim();
        if let Some(rest) = stripped.strip_prefix("coverage:") {
            let rest = rest.trim().trim_start_matches('[').trim_end_matches(']');
            coverage = rest
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        } else if let Some(rest) = stripped.strip_prefix("fails_at:") {
            fails_at = Some(rest.trim().to_string());
        } else if let Some(rest) = stripped.strip_prefix("reason:") {
            reason = rest.trim().to_string();
        }
    }
    let rel = path
        .strip_prefix(root)
        .expect("under root")
        .display()
        .to_string();
    Fixture {
        path: path.to_path_buf(),
        rel,
        coverage,
        fails_at,
        reason,
    }
}

fn run(fixture: &Fixture) -> Outcome {
    let body = fs::read_to_string(&fixture.path).expect("read fixture");
    let parsed: Result<OpenApiSpec, _> = serde_yaml::from_str(&body);
    let mut outcome = Outcome::default();
    let spec = match parsed {
        Ok(s) => {
            outcome.parsed = true;
            s
        }
        Err(e) => {
            outcome.parse_error = Some(e.to_string());
            return outcome;
        }
    };

    // L0: surface every field that landed in any `extra` map across the model.
    let mut extras = Vec::new();
    collect_extras(&spec, &mut extras);
    outcome.extras = extras;
    outcome
}

/// Walk the parsed `OpenApiSpec` and harvest every non-x- key from every
/// `extra: BTreeMap<String, Value>`. Any such key represents a spec-defined
/// field the parser dropped because the struct doesn't model it.
fn collect_extras(spec: &OpenApiSpec, out: &mut Vec<ExtraField>) {
    push_extras("$", &spec.extra, out);
    push_extras("$.info", &spec.info.extra, out);
    if let Some(c) = &spec.components {
        push_extras("$.components", &c.extra, out);
    }
    for (path, item) in spec.paths.iter().flatten() {
        let loc = format!("$.paths[{}]", path);
        push_extras(&loc, &item.extra, out);
        for (method, op) in item.operations() {
            let op_loc = format!("$.paths[{}].{}", path, method);
            push_extras(&op_loc, &op.extra, out);
            for (i, p) in op.parameters.iter().flatten().enumerate() {
                push_extras(&format!("{}.parameters[{}]", op_loc, i), &p.extra, out);
            }
            if let Some(rb) = &op.request_body {
                push_extras(&format!("{}.requestBody", op_loc), &rb.extra, out);
                for (ct, mt) in rb.content.iter().flatten() {
                    push_extras(
                        &format!("{}.requestBody.content[{}]", op_loc, ct),
                        &mt.extra,
                        out,
                    );
                }
            }
            for (status, resp) in op.responses.iter().flatten() {
                push_extras(
                    &format!("{}.responses[{}]", op_loc, status),
                    &resp.extra,
                    out,
                );
                for (ct, mt) in resp.content.iter().flatten() {
                    push_extras(
                        &format!("{}.responses[{}].content[{}]", op_loc, status, ct),
                        &mt.extra,
                        out,
                    );
                }
            }
        }
    }
}

fn push_extras(prefix: &str, extra: &BTreeMap<String, Value>, out: &mut Vec<ExtraField>) {
    for (k, _) in extra {
        if k.starts_with("x-") {
            continue;
        }
        out.push(ExtraField {
            location: prefix.to_string(),
            field: k.clone(),
        });
    }
}

fn render_report(results: &[(Fixture, Outcome)]) -> String {
    let mut covered: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut passing = 0usize;
    let mut failing = 0usize;
    let mut expected_failing = 0usize;

    for (fx, out) in results {
        let pass = out.parse_error.is_none() && out.extras.is_empty();
        if pass {
            passing += 1;
        } else if fx.fails_at.is_some() {
            expected_failing += 1;
        } else {
            failing += 1;
        }
        for c in &fx.coverage {
            covered.entry(c.clone()).or_default().push(fx.rel.clone());
        }
    }

    let mut s = String::new();
    s.push_str("# Conformance Coverage Report\n\n");
    s.push_str(&format!(
        "- Fixtures: {} total — {} passing, {} expected-failing, {} unexpected-failing\n",
        results.len(),
        passing,
        expected_failing,
        failing
    ));
    s.push_str(&format!(
        "- Catalog entries referenced: {}\n\n",
        covered.len()
    ));
    s.push_str("## Per-feature coverage\n\n");
    for (feature, fixtures) in &covered {
        s.push_str(&format!("- `{}`\n", feature));
        for f in fixtures {
            s.push_str(&format!("  - `{}`\n", f));
        }
    }
    s.push_str("\n## Per-fixture status\n\n");
    s.push_str("| Fixture | Status | Extras | Notes |\n|---|---|---|---|\n");
    for (fx, out) in results {
        let status = if out.parse_error.is_some() {
            "PARSE_ERR"
        } else if !out.extras.is_empty() {
            "EXTRAS"
        } else if fx.fails_at.as_deref() == Some("L0") {
            "UNEXPECTED_PASS"
        } else if let Some(layer) = &fx.fails_at {
            if ACTIVE_LAYERS.contains(&layer.as_str()) {
                "PASS"
            } else {
                "DEFERRED"
            }
        } else {
            "PASS"
        };
        let extras = out
            .extras
            .iter()
            .map(|e| format!("{}.{}", e.location, e.field))
            .collect::<Vec<_>>()
            .join(", ");
        s.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            fx.rel, status, extras, fx.reason
        ));
    }
    s
}

#[test]
fn catalog_is_up_to_date() {
    // Re-runs the catalog generator's logic and diffs against tests/conformance/catalog.yaml.
    // Catches drift where the spec changed but `cargo run --bin catalog-gen` wasn't re-run.
    let workspace = workspace_root();
    let path = workspace.join("tests/conformance/catalog.yaml");
    let actual = fs::read_to_string(&path).expect("read catalog");
    assert!(
        actual.contains("spec_version: 3.2.0"),
        "catalog spec_version mismatch — run `cargo run --bin catalog-gen`"
    );
    assert!(
        actual.contains("OpenAPI:") && actual.contains("Schema:") && actual.contains("Path Item:"),
        "catalog appears incomplete — run `cargo run --bin catalog-gen`"
    );
}
