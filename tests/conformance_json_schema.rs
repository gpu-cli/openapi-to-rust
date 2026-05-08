//! JSON Schema 2020-12 conformance runner.
//!
//! Walks the json-schema-org/JSON-Schema-Test-Suite submodule (see
//! tests/conformance/external/json-schema-test-suite) and runs each test
//! file's schemas through our `Schema` deserializer.
//!
//! Initial scope: parse-only. Each test case schema must deserialize without
//! error and round-trip back to JSON without losing keys. We are not yet
//! validating data instances against schemas — that's a future layer once the
//! schema model is complete.
//!
//! Results are written to tests/conformance/json-schema-2020-12-report.md when
//! `CONFORMANCE_REPORT=1` is set.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use openapi_to_rust::openapi::Schema;
use serde_json::Value;

const SUITE_REL: &str = "tests/conformance/external/json-schema-test-suite/tests/draft2020-12";

/// Keywords whose support we have not yet implemented. Test files focused on
/// these are recorded as `INTENTIONALLY_SKIPPED` rather than failing the
/// build. Each entry must justify itself in `reason`.
const SKIP_LIST: &[(&str, &str)] = &[
    (
        "dynamicRef.json",
        "$dynamicRef/$dynamicAnchor not modeled (only obsolete $recursiveRef)",
    ),
    ("vocabulary.json", "$vocabulary handling not modeled"),
    ("unknownKeyword.json", "deny_unknown_fields not yet enabled"),
];

#[derive(Debug)]
struct Suite {
    file_name: String,
    cases: Vec<Case>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct Case {
    description: String,
    schema: Value,
}

#[derive(Debug, Default)]
struct Tally {
    parsed: usize,
    parse_failed: Vec<String>,
    round_trip_lossy: Vec<String>,
}

#[test]
fn json_schema_2020_12_parse_corpus() {
    let workspace = workspace_root();
    let suite_dir = workspace.join(SUITE_REL);
    if !suite_dir.exists() {
        // Submodule not cloned. CI must `git submodule update --init`. Locally,
        // we'd rather skip than break developer ergonomics.
        eprintln!(
            "json-schema-test-suite not present at {} — skipping (run `git submodule update --init`)",
            suite_dir.display()
        );
        return;
    }

    let suites = load_suites(&suite_dir);
    assert!(
        !suites.is_empty(),
        "no test files found in {}",
        suite_dir.display()
    );

    let skip: BTreeMap<&str, &str> = SKIP_LIST.iter().copied().collect();
    let mut by_file: BTreeMap<String, Tally> = BTreeMap::new();
    let mut skipped: Vec<(String, &str)> = Vec::new();

    for suite in suites {
        if let Some(reason) = skip.get(suite.file_name.as_str()) {
            skipped.push((suite.file_name.clone(), reason));
            continue;
        }
        let mut tally = Tally::default();
        for case in suite.cases {
            match serde_json::from_value::<Schema>(case.schema.clone()) {
                Ok(parsed) => {
                    tally.parsed += 1;
                    let round = serde_json::to_value(&parsed).expect("schema serializes");
                    if !json_keys_subset(&case.schema, &round) {
                        tally.round_trip_lossy.push(case.description);
                    }
                }
                Err(e) => {
                    tally
                        .parse_failed
                        .push(format!("{} ({})", case.description, e));
                }
            }
        }
        by_file.insert(suite.file_name, tally);
    }

    if std::env::var_os("CONFORMANCE_REPORT").is_some() {
        let report = render_report(&by_file, &skipped);
        let out = workspace.join("tests/conformance/json-schema-2020-12-report.md");
        fs::write(&out, report).expect("write");
        eprintln!("wrote {}", out.display());
    }

    let total_parsed: usize = by_file.values().map(|t| t.parsed).sum();
    let total_failed: usize = by_file.values().map(|t| t.parse_failed.len()).sum();
    let total_lossy: usize = by_file.values().map(|t| t.round_trip_lossy.len()).sum();
    eprintln!(
        "JSON Schema 2020-12: {} files run, {} cases parsed, {} parse-failed, {} round-trip-lossy, {} files skipped",
        by_file.len(),
        total_parsed,
        total_failed,
        total_lossy,
        skipped.len(),
    );

    // Initial gate: we don't yet enforce a parse pass-rate threshold here —
    // the report is informational. As the schema layer matures we'll ratchet
    // this up: e.g. require parse_failed.len() < N, then == 0, then enforce
    // round-trip losslessness.
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_suites(dir: &Path) -> Vec<Suite> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("readdir suite") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read suite file");
        let groups: Value = serde_json::from_str(&body).expect("parse suite file");
        let mut cases = Vec::new();
        if let Some(arr) = groups.as_array() {
            for group in arr {
                let schema = group
                    .get("schema")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                let desc = group
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("(no description)")
                    .to_string();
                cases.push(Case {
                    description: desc,
                    schema,
                });
            }
        }
        out.push(Suite {
            file_name: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string(),
            cases,
        });
    }
    out.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    out
}

/// Returns true if every top-level object key in `expected` is present in
/// `actual` (recursively). Used as a cheap "we kept the keys" check; a full
/// equality would require canonicalization and is out of scope here.
fn json_keys_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => e
            .iter()
            .all(|(k, ev)| a.get(k).map(|av| json_keys_subset(ev, av)).unwrap_or(false)),
        _ => true,
    }
}

fn render_report(by_file: &BTreeMap<String, Tally>, skipped: &[(String, &str)]) -> String {
    let mut s = String::new();
    s.push_str("# JSON Schema 2020-12 Parse Report\n\n");
    s.push_str(
        "Source: tests/conformance/external/json-schema-test-suite (json-schema-org).\n\
         This report covers the **parse** layer only — we deserialize each test\n\
         schema into our `Schema` model and check that no top-level keys are\n\
         lost on round-trip. Validation against data instances is a future layer.\n\n",
    );
    s.push_str("| File | Parsed | Parse-failed | Round-trip-lossy |\n|---|---:|---:|---:|\n");
    for (file, t) in by_file {
        s.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            file,
            t.parsed,
            t.parse_failed.len(),
            t.round_trip_lossy.len()
        ));
    }
    if !skipped.is_empty() {
        s.push_str("\n## Intentionally skipped\n\n");
        for (f, reason) in skipped {
            s.push_str(&format!("- `{}` — {}\n", f, reason));
        }
    }
    s.push_str("\n## Detailed parse failures\n\n");
    for (file, t) in by_file {
        if t.parse_failed.is_empty() {
            continue;
        }
        s.push_str(&format!("### `{}`\n", file));
        for f in &t.parse_failed {
            s.push_str(&format!("- {}\n", f));
        }
    }
    s
}
