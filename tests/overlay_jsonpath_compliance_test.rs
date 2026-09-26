//! Official RFC 9535 compliance corpus, vendored from
//! https://github.com/jsonpath-standard/jsonpath-compliance-test-suite
//! (revision 9d1a415a53f5dfb291bc874823892e49174e38eb; BSD-2 license beside fixture).
#![allow(clippy::unwrap_used)]

use serde_json::Value;
use serde_json_path::JsonPath;

#[test]
fn official_jsonpath_compliance_corpus() {
    let corpus: Value = serde_json::from_str(include_str!("fixtures/jsonpath/cts.json")).unwrap();
    let tests = corpus["tests"].as_array().unwrap();
    let mut failures = Vec::new();
    for case in tests {
        let name = case["name"].as_str().unwrap();
        let selector = case["selector"].as_str().unwrap();
        let parsed = JsonPath::parse(selector);
        if case["invalid_selector"] == true {
            if parsed.is_ok() {
                failures.push(format!("{name}: accepted invalid selector {selector}"));
            }
            continue;
        }
        let path = match parsed {
            Ok(path) => path,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        let selected = path.query_located(&case["document"]);
        let result = Value::Array(selected.nodes().cloned().collect());
        let locations = Value::Array(
            selected
                .locations()
                .map(|location| Value::String(normalized_path(location)))
                .collect(),
        );
        let matches = if let Some(results) = case["results"].as_array() {
            results.contains(&result)
        } else {
            result == case["result"]
        };
        let locations_match = if let Some(paths) = case["results_paths"].as_array() {
            paths.contains(&locations)
        } else {
            locations == case["result_paths"]
        };
        if !locations_match {
            failures.push(format!("{name}: wrong located-query paths: {locations}"));
        }
        if !matches {
            failures.push(format!(
                "{name}: expected {:?} / {:?}, got {result}",
                case.get("result"),
                case.get("results")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures out of {} cases:\n{}",
        failures.len(),
        tests.len(),
        failures.join("\n")
    );
}

// serde_json_path's Display omits RFC 9535 escapes for member names. Overlay
// mutation uses structured locations and JSON Pointers, so compare the actual
// structured locations with the corpus using an independent canonical encoder.
fn normalized_path(path: &serde_json_path::NormalizedPath<'_>) -> String {
    let mut result = "$".to_string();
    for element in path.iter() {
        match element {
            serde_json_path::PathElement::Index(index) => result.push_str(&format!("[{index}]")),
            serde_json_path::PathElement::Name(name) => {
                result.push_str("['");
                for ch in name.chars() {
                    match ch {
                        '\'' => result.push_str("\\'"),
                        '\\' => result.push_str("\\\\"),
                        '\u{08}' => result.push_str("\\b"),
                        '\u{0c}' => result.push_str("\\f"),
                        '\n' => result.push_str("\\n"),
                        '\r' => result.push_str("\\r"),
                        '\t' => result.push_str("\\t"),
                        ch if ch <= '\u{1f}' => result.push_str(&format!("\\u{:04x}", ch as u32)),
                        ch => result.push(ch),
                    }
                }
                result.push_str("']");
            }
        }
    }
    result
}
