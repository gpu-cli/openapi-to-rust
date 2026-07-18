//! Spec-source policy and document parsing shared by the CLI, library
//! consumers, and the WASM playground build.
//!
//! Everything in this module is pure: URL policy checks build on `url::Url`
//! and document parsing goes through `serde_yaml`/`serde_json` in memory. The
//! I/O that actually fetches or reads a spec lives in [`crate::cli`], which is
//! gated behind the `cli` feature.

/// Whether an input string names a supported remote OpenAPI source.
pub fn is_remote_spec(input: &str) -> bool {
    url::Url::parse(input).is_ok_and(|url| matches!(url.scheme(), "https" | "http"))
}

/// Parse and enforce the remote-source transport policy.
pub fn validate_remote_spec_url(input: &str) -> Result<url::Url, String> {
    let url =
        url::Url::parse(input).map_err(|error| format!("invalid remote OpenAPI URL: {error}"))?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("remote OpenAPI URLs must not contain embedded credentials".to_string());
    }
    match url.scheme() {
        "https" => Ok(url),
        "http" if is_loopback_host(url.host_str()) => Ok(url),
        "http" => Err(
            "remote OpenAPI URLs must use HTTPS (plain HTTP is allowed only for localhost/loopback)"
                .to_string(),
        ),
        scheme => Err(format!(
            "unsupported OpenAPI URL scheme `{scheme}`; use HTTPS or a local file path"
        )),
    }
}

/// Remove URL credentials, query strings, and fragments before recording a
/// source label in generated code. Local paths are retained as supplied.
pub fn sanitize_source_provenance(input: &str) -> String {
    let sanitize_controls = |value: &str| {
        value
            .chars()
            .map(|character| {
                if character.is_control() {
                    '�'
                } else {
                    character
                }
            })
            .collect::<String>()
    };
    let Ok(mut url) = url::Url::parse(input) else {
        return sanitize_controls(input);
    };
    if !matches!(url.scheme(), "https" | "http") {
        return sanitize_controls(input);
    }
    let query_was_redacted = url.query().is_some();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    let mut label = url.to_string();
    if query_was_redacted {
        label.push_str(" (query redacted)");
    }
    sanitize_controls(&label)
}
fn is_loopback_host(host: Option<&str>) -> bool {
    match host {
        Some("localhost") => true,
        Some(host) => host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback()),
        None => false,
    }
}

/// Parse the `openapi` version string into (major, minor). Tolerates patch and
/// build-metadata suffixes. Returns None for unrecognised input.
pub fn parse_oas_version(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor_raw = parts.next()?;
    let minor_digits: String = minor_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let minor = minor_digits.parse().ok()?;
    Some((major, minor))
}

pub fn parse_spec(
    content: &str,
    input: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
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

/// Validate the `openapi` version field of a parsed document.
///
/// Returns an optional warning for experimental versions (3.2) and an error
/// for unsupported or missing versions.
pub fn validate_oas_document(value: &serde_json::Value) -> Result<Option<String>, String> {
    let version = value
        .get("openapi")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match parse_oas_version(version) {
        Some((3, 0 | 1)) => Ok(None),
        Some((3, 2)) => Ok(Some(format!(
            "OpenAPI {version} support is experimental; some 3.2-only features are not generated"
        ))),
        Some((major, minor)) => Err(format!(
            "unsupported OpenAPI version {major}.{minor} ({version:?}); expected 3.0, 3.1, or experimental 3.2"
        )),
        None => {
            let hint = if value.get("swagger").is_some() {
                " (the document appears to be Swagger 2.0)"
            } else {
                ""
            };
            Err(format!("missing or unrecognized `openapi` version{hint}"))
        }
    }
}
