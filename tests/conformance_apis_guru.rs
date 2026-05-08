//! APIs.guru openapi-directory smoke runner. Off by default — opt-in via:
//!
//!   APIS_GURU_SMOKE=1 cargo test --test conformance_apis_guru
//!
//! This corpus is too large for CI's default lane. Use it as the broad
//! real-world canary nightly or before tagging a release.
//!
//! Initial scope: parse every spec, report how many parse + how many drop
//! fields into `extra` maps. We do NOT yet attempt code generation here —
//! that would need real-spec-quality codegen to be meaningful.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use openapi_to_rust::openapi::OpenApiSpec;

const APIS_GURU_REL: &str = "tests/conformance/external/apis-guru/APIs";

#[test]
fn apis_guru_smoke() {
    if std::env::var_os("APIS_GURU_SMOKE").is_none() {
        eprintln!("APIS_GURU_SMOKE not set — skipping (run tests/conformance/external/apis-guru-sync.sh first, then export APIS_GURU_SMOKE=1)");
        return;
    }

    let workspace = workspace_root();
    let root = workspace.join(APIS_GURU_REL);
    if !root.exists() {
        eprintln!(
            "APIs.guru corpus not present at {} — run tests/conformance/external/apis-guru-sync.sh",
            root.display()
        );
        return;
    }

    let mut specs = Vec::new();
    walk(&root, &mut specs);
    assert!(!specs.is_empty(), "no specs under {}", root.display());

    let mut parse_failed: Vec<String> = Vec::new();
    let mut by_provider: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for path in &specs {
        let provider = provider_label(path);
        let entry = by_provider.entry(provider.clone()).or_insert((0, 0));
        let body = match fs::read_to_string(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        match parse(&body, path) {
            Ok(_spec) => entry.0 += 1,
            Err(e) => {
                entry.1 += 1;
                parse_failed.push(format!("{}: {}", path.display(), e));
            }
        }
    }

    eprintln!(
        "APIs.guru: {} specs scanned, {} parse-failed",
        specs.len(),
        parse_failed.len()
    );

    if std::env::var_os("CONFORMANCE_REPORT").is_some() {
        let mut s = String::from("# APIs.guru Parse Smoke Report\n\n");
        s.push_str(&format!("Total specs: {}\n\n", specs.len()));
        s.push_str("| Provider | OK | Failed |\n|---|---:|---:|\n");
        for (p, (ok, fail)) in &by_provider {
            s.push_str(&format!("| `{}` | {} | {} |\n", p, ok, fail));
        }
        if !parse_failed.is_empty() {
            s.push_str("\n## Parse failures (first 50)\n\n");
            for f in parse_failed.iter().take(50) {
                s.push_str(&format!("- {}\n", f));
            }
        }
        let out = workspace.join("tests/conformance/apis-guru-report.md");
        fs::write(&out, s).expect("write");
        eprintln!("wrote {}", out.display());
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if (ext == "yaml" || ext == "json") && (name == "openapi.yaml" || name == "openapi.json") {
                out.push(path);
            }
        }
    }
}

fn parse(body: &str, path: &Path) -> Result<OpenApiSpec, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext == "yaml" {
        serde_yaml::from_str(body).map_err(|e| e.to_string())
    } else {
        serde_json::from_str(body).map_err(|e| e.to_string())
    }
}

fn provider_label(path: &Path) -> String {
    // .../APIs/<provider>/<api>/<version>/openapi.yaml
    let mut comps: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    while let Some(first) = comps.first() {
        if *first == "APIs" {
            comps.remove(0);
            break;
        }
        comps.remove(0);
    }
    comps.first().map(|s| s.to_string()).unwrap_or_else(|| "?".to_string())
}
