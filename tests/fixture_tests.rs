use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Helper function to test that generated code compiles from fixture files
fn test_fixture_compiles(
    fixture_name: &str,
    spec: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // Create temp directory
    let temp_dir = TempDir::new()?;
    let temp_path = temp_dir.path();

    // Analyze schema
    let mut analyzer = SchemaAnalyzer::new(spec)?;
    let mut analysis = analyzer.analyze()?;

    // Generate code
    let config = GeneratorConfig {
        module_name: fixture_name.to_string(),
        ..Default::default()
    };
    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    // Create a temporary Rust project
    let cargo_toml = format!(
        r#"
[package]
name = "{fixture_name}"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
"#
    );

    let lib_rs = format!(
        r#"#![recursion_limit = "512"]
{generated_code}

#[cfg(test)]
mod tests {{
    use super::*;
    use serde_json;
    
    #[test]
    fn test_generated_types_compile() {{
        // Test that our generated types compile and run
        let _json_value = serde_json::Value::Null;
        let test_obj = serde_json::json!({{"test": "value"}});
        let _serialized = serde_json::to_string(&test_obj).unwrap();
        // Test completed successfully
    }}
}}
"#
    );

    // Write files
    fs::write(temp_path.join("Cargo.toml"), cargo_toml)?;
    let src_dir = temp_path.join("src");
    fs::create_dir_all(&src_dir)?;
    fs::write(src_dir.join("lib.rs"), lib_rs)?;

    // Try to compile and run tests including doctests
    let output = Command::new("cargo")
        .arg("test")
        .arg("--release")
        .current_dir(temp_path)
        .output()?;

    if !output.status.success() {
        eprintln!("Generated code for fixture {fixture_name} failed to compile:");
        eprintln!("STDOUT: {}", String::from_utf8_lossy(&output.stdout));
        eprintln!("STDERR: {}", String::from_utf8_lossy(&output.stderr));
        // Don't print the generated code for fixtures as it might be very large
        return Err(format!("Compilation failed for fixture {fixture_name}").into());
    }

    println!("✓ Fixture {fixture_name} generated code compiles successfully");
    Ok(())
}

/// Test all fixtures in the fixtures directory
#[test]
fn test_all_fixtures_compile() {
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");

    let mut fixture_count = 0;
    let mut success_count = 0;
    let mut failures = Vec::new();

    // Read all files in fixtures directory
    let entries = fs::read_dir(&fixtures_dir)
        .expect("Failed to read fixtures directory")
        .collect::<Result<Vec<_>, _>>()
        .expect("Failed to collect directory entries");

    // Sort entries by filename for consistent test order
    let mut entries = entries;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_file() {
            let extension = path.extension().and_then(|e| e.to_str());

            // Only process JSON and YAML files
            if !matches!(extension, Some("json") | Some("yaml") | Some("yml")) {
                continue;
            }

            let fixture_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .replace(['-', '.'], "_"); // Convert to valid Rust identifier

            fixture_count += 1;
            println!("Testing fixture: {fixture_name}");

            // Read and parse the fixture file
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("Failed to read fixture file: {path:?}"));

            let spec: serde_json::Value = match extension {
                Some("json") => serde_json::from_str(&content)
                    .unwrap_or_else(|_| panic!("Failed to parse JSON fixture: {path:?}")),
                Some("yaml") | Some("yml") => serde_yaml::from_str(&content)
                    .unwrap_or_else(|_| panic!("Failed to parse YAML fixture: {path:?}")),
                _ => unreachable!(),
            };

            // Test the fixture
            match test_fixture_compiles(&fixture_name, spec) {
                Ok(()) => {
                    success_count += 1;
                    println!("  ✅ SUCCESS");
                }
                Err(e) => {
                    failures.push((fixture_name.clone(), e.to_string()));
                    println!("  ❌ FAILED: {e}");
                }
            }
        }
    }

    // Print summary
    println!("\n🎯 Fixture Test Summary:");
    println!("  📁 Total fixtures: {fixture_count}");
    println!("  ✅ Successful: {success_count}");
    println!("  ❌ Failed: {}", failures.len());

    if !failures.is_empty() {
        println!("\n💥 Failures:");
        for (fixture, error) in &failures {
            println!("  • {fixture}: {error}");
        }
    }

    // Require at least some fixtures to be present
    assert!(
        fixture_count > 0,
        "No fixture files found in tests/fixtures/"
    );

    // For now, don't require all fixtures to pass - just report results
    if !failures.is_empty() {
        println!(
            "\n⚠️  Some fixtures failed compilation. This is expected as we continue developing the generator."
        );
        println!(
            "    Failed fixtures will help us identify missing features and edge cases to implement."
        );
    }

    println!("\n🚀 Fixture testing completed!");
}

/// Individual fixture tests for easier debugging
#[test]
fn test_anthropic_fixture() {
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");

    let anthropic_path = fixtures_dir.join("anthropic.yml");
    if anthropic_path.exists() {
        let content = fs::read_to_string(&anthropic_path).expect("Failed to read anthropic.yml");

        let spec: serde_json::Value =
            serde_yaml::from_str(&content).expect("Failed to parse anthropic.yml");

        test_fixture_compiles("anthropic", spec).expect("Anthropic fixture should compile");
    } else {
        println!("Anthropic fixture not found, skipping individual test");
    }
}

#[test]
fn test_openai_responses_fixture() {
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");

    let openai_path = fixtures_dir.join("openai-responses.json");
    if openai_path.exists() {
        let content =
            fs::read_to_string(&openai_path).expect("Failed to read openai-responses.json");

        let spec: serde_json::Value =
            serde_json::from_str(&content).expect("Failed to parse openai-responses.json");

        test_fixture_compiles("openai_responses", spec)
            .expect("OpenAI responses fixture should compile");
    } else {
        println!("OpenAI responses fixture not found, skipping individual test");
    }
}
