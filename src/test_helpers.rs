use crate::{CodeGenerator, GeneratedFile, GeneratorConfig, SchemaAnalyzer};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// Result of a generation test
pub struct GenerationTestResult {
    /// Temporary directory containing all generated files
    pub temp_dir: TempDir,
    /// Path to the test project directory
    pub project_dir: PathBuf,
    /// Path to the generated source directory
    pub generated_src_dir: PathBuf,
    /// Generated files
    pub files: Vec<GeneratedFile>,
    /// Compilation output (if compilation was run)
    pub compilation_output: Option<std::process::Output>,
    /// Path to Cargo.toml
    pub cargo_toml_path: PathBuf,
}

impl GenerationTestResult {
    /// Read a generated file by name
    pub fn read_file(&self, filename: &str) -> std::io::Result<String> {
        let path = self.generated_src_dir.join(filename);
        fs::read_to_string(path)
    }

    /// Check if compilation succeeded
    pub fn compiled_successfully(&self) -> bool {
        self.compilation_output
            .as_ref()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get compilation errors
    pub fn compilation_errors(&self) -> String {
        self.compilation_output
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stderr).to_string())
            .unwrap_or_default()
    }

    /// Get test output
    pub fn test_output(&self) -> Option<String> {
        self.compilation_output
            .as_ref()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    }
}

/// Configuration for a generation test
pub struct GenerationTest {
    /// Name of the test (used for project name)
    pub name: String,
    /// OpenAPI specification
    pub spec: Value,
    /// Generator configuration (optional overrides)
    pub config_overrides: Option<GeneratorConfigOverrides>,
    /// Test scenarios to generate
    pub test_scenarios: Vec<TestScenario>,
    /// Whether to run tests after compilation (only if compilation is enabled)
    pub run_tests: bool,
    /// Whether to compile the generated code (false by default, must opt-in)
    pub compile: bool,
    /// Additional dependencies for Cargo.toml
    pub extra_dependencies: Vec<(String, String)>,
}

/// A test scenario to generate
#[derive(Clone)]
pub struct TestScenario {
    /// Name of the test function
    pub name: String,
    /// Type to test
    pub target_type: String,
    /// Test behavior
    pub behavior: TestBehavior,
}

/// Different test behaviors we can generate
#[derive(Clone)]
pub enum TestBehavior {
    /// Test serialization/deserialization round trip
    RoundTrip {
        /// JSON value to test with
        json: Value,
        /// Expected field values to assert (field_name -> expected_value)
        assertions: Vec<(String, Value)>,
    },
    /// Test creating an instance with specific values
    Construction {
        /// Field values to set (field_name -> value_expr)
        /// Value expressions are simple literals or enum variants
        fields: Vec<(String, FieldValue)>,
        /// Assertions to make on the created instance
        assertions: Vec<ConstructionAssertion>,
    },
    /// Test that a type exists and compiles
    CompileOnly,
    /// Test enum variant matching
    EnumMatch {
        /// JSON to deserialize
        json: Value,
        /// Expected variant name
        expected_variant: String,
        /// Fields to check in the variant (field_name -> expected_value)
        variant_assertions: Vec<(String, Value)>,
    },
}

/// Value to use when constructing a field
#[derive(Clone)]
pub enum FieldValue {
    /// String literal
    String(String),
    /// Integer literal
    Integer(i64),
    /// Float literal
    Float(f64),
    /// Boolean literal
    Boolean(bool),
    /// Null value
    Null,
    /// Enum variant (e.g., "MyEnum::Variant")
    EnumVariant(String),
    /// Array of values
    Array(Vec<FieldValue>),
    /// Struct construction (type_name, fields)
    Struct(String, Vec<(String, FieldValue)>),
}

/// Assertion to make on a constructed value
#[derive(Clone)]
pub enum ConstructionAssertion {
    /// Assert serialized JSON contains a string
    JsonContains(String),
    /// Assert a field equals a value
    FieldEquals(String, Value),
    /// Assert successful serialization
    CanSerialize,
}

/// Overrides for generator configuration
pub struct GeneratorConfigOverrides {
    pub module_name: Option<String>,
    pub enable_sse_client: Option<bool>,
    pub enable_async_client: Option<bool>,
}

impl GenerationTest {
    /// Create a new test with the given name and spec
    pub fn new(name: impl Into<String>, spec: Value) -> Self {
        Self {
            name: name.into(),
            spec,
            ..Default::default()
        }
    }

    /// Enable compilation for this test
    pub fn with_compilation(mut self) -> Self {
        self.compile = true;
        self
    }

    /// Enable compilation only if OPENAPI_GEN_COMPILE_TESTS is set
    pub fn with_env_compilation(mut self) -> Self {
        self.compile = should_compile_tests();
        self
    }

    /// Add a test scenario
    pub fn with_scenario(mut self, scenario: TestScenario) -> Self {
        self.test_scenarios.push(scenario);
        self
    }

    /// Set whether to run tests (only applies if compilation is enabled)
    pub fn run_tests(mut self, run: bool) -> Self {
        self.run_tests = run;
        self
    }
}

impl Default for GenerationTest {
    fn default() -> Self {
        Self {
            name: "test".to_string(),
            spec: Value::Null,
            config_overrides: None,
            test_scenarios: vec![],
            run_tests: true,
            extra_dependencies: vec![],
            compile: false, // Opt-in compilation
        }
    }
}

/// Check if we should run full compilation tests (via environment variable)
pub fn should_compile_tests() -> bool {
    env::var("OPENAPI_GEN_COMPILE_TESTS")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Fast test function that only validates syntax (no compilation)
/// When called from tests, automatically creates/verifies snapshots
pub fn test_generation(name: &str, spec: Value) -> Result<String, Box<dyn std::error::Error>> {
    // Analyze the spec
    let mut analyzer = SchemaAnalyzer::new(spec)?;
    let mut analysis = analyzer.analyze()?;

    // Generate code
    let config = GeneratorConfig {
        module_name: name.to_string(),
        ..Default::default()
    };
    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    // The code is already validated by syn in the generator

    // Automatically assert snapshot when insta is available
    insta::assert_snapshot!(name, &generated_code);

    Ok(generated_code)
}

/// Run a complete generation test
pub fn run_generation_test(
    test: GenerationTest,
) -> Result<GenerationTestResult, Box<dyn std::error::Error>> {
    // Create temporary directory
    let temp_dir = TempDir::new()?;
    let project_dir = temp_dir.path().join(&test.name);
    fs::create_dir(&project_dir)?;

    // Set up paths
    let src_dir = project_dir.join("src");
    fs::create_dir(&src_dir)?;
    let generated_src_dir = src_dir.join("generated");
    fs::create_dir(&generated_src_dir)?;

    // Analyze the spec
    let mut analyzer = SchemaAnalyzer::new(test.spec.clone())?;
    let analysis = analyzer.analyze()?;

    // Configure generator
    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"), // Not used when we pass analysis directly
        output_dir: generated_src_dir.clone(),
        module_name: test
            .config_overrides
            .as_ref()
            .and_then(|o| o.module_name.clone())
            .unwrap_or_else(|| "generated".to_string()),
        enable_sse_client: test
            .config_overrides
            .as_ref()
            .and_then(|o| o.enable_sse_client)
            .unwrap_or(false),
        enable_async_client: test
            .config_overrides
            .as_ref()
            .and_then(|o| o.enable_async_client)
            .unwrap_or(false),
        enable_specta: false,
        type_mappings: {
            let mut mappings = std::collections::BTreeMap::new();
            mappings.insert("integer".to_string(), "i64".to_string());
            mappings.insert("number".to_string(), "f64".to_string());
            mappings.insert("string".to_string(), "String".to_string());
            mappings.insert("boolean".to_string(), "bool".to_string());
            mappings
        },
        streaming_config: None,
        nullable_field_overrides: std::collections::BTreeMap::new(),
        extensible_enum_overrides: std::collections::BTreeMap::new(),
        schema_extensions: vec![],
        http_client_config: None,
        retry_config: None,
        tracing_enabled: true,
        auth_config: None,
        enable_registry: false,
        registry_only: false,
        types: crate::type_mapping::TypeMappingConfig::default(),
    };

    // Generate code
    let generator = CodeGenerator::new(config);
    let mut analysis_mut = analysis;
    let types_content = generator.generate(&mut analysis_mut)?;

    // Create files list with just the types file for now
    let files = vec![GeneratedFile {
        path: "types.rs".into(),
        content: types_content.clone(),
    }];

    // Write generated files
    for file in &files {
        let dest_path = generated_src_dir.join(&file.path);
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest_path, &file.content)?;
    }

    // Create Cargo.toml
    let mut dependencies = vec![
        ("serde", r#"{ version = "1.0", features = ["derive"] }"#),
        ("serde_json", r#""1.0""#),
        ("async-trait", r#""0.1""#),
        (
            "reqwest",
            r#"{ version = "0.12", features = ["json", "stream"] }"#,
        ),
        ("futures-util", r#""0.3""#),
        ("tokio", r#"{ version = "1.0", features = ["full"] }"#),
        ("tracing", r#""0.1""#),
        // Q2 typed-scalar deps (default-on; harmless when unused).
        ("chrono", r#"{ version = "0.4", features = ["serde"] }"#),
        ("uuid", r#"{ version = "1", features = ["serde", "v4"] }"#),
        ("url", r#"{ version = "2", features = ["serde"] }"#),
        ("bytes", r#"{ version = "1", features = ["serde"] }"#),
        ("base64", r#""0.22""#),
    ];

    // Add extra dependencies
    for (name, version) in &test.extra_dependencies {
        dependencies.push((name.as_str(), version.as_str()));
    }

    let deps_str = dependencies
        .iter()
        .map(|(name, ver)| format!("{name} = {ver}"))
        .collect::<Vec<_>>()
        .join("\n");

    let cargo_toml = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
{}
"#,
        test.name.replace('-', "_"),
        deps_str
    );

    let cargo_toml_path = project_dir.join("Cargo.toml");
    fs::write(&cargo_toml_path, cargo_toml)?;

    // Create generated/mod.rs
    fs::write(generated_src_dir.join("mod.rs"), "pub mod types;\n")?;

    // Create lib.rs with tests
    let lib_rs = create_lib_rs(&test, &files);
    fs::write(src_dir.join("lib.rs"), lib_rs)?;

    // Only compile if explicitly requested
    let compilation_output = if !test.compile {
        None
    } else if test.run_tests {
        // Run tests if requested
        Some(
            Command::new("cargo")
                .arg("test")
                .arg("--")
                .arg("--nocapture")
                .current_dir(&project_dir)
                .env("RUST_BACKTRACE", "1")
                .output()?,
        )
    } else {
        // Just check compilation
        Some(
            Command::new("cargo")
                .arg("check")
                .current_dir(&project_dir)
                .env("RUST_BACKTRACE", "1")
                .output()?,
        )
    };

    // Debug: print generated types for failing tests
    if let Some(ref output) = compilation_output {
        if !output.status.success() {
            if let Ok(types_content) = fs::read_to_string(generated_src_dir.join("types.rs")) {
                eprintln!("Generated types.rs:\n{types_content}");
            }
        }
    }

    Ok(GenerationTestResult {
        temp_dir,
        project_dir,
        generated_src_dir,
        files,
        compilation_output,
        cargo_toml_path,
    })
}

fn create_lib_rs(test: &GenerationTest, _generated_files: &[GeneratedFile]) -> String {
    let mut lib_content = String::from("pub mod generated;\n\n");

    // Add test module
    lib_content.push_str("#[cfg(test)]\nmod tests {\n");
    lib_content.push_str("    use super::generated::types::*;\n");
    lib_content.push_str("    use serde_json;\n\n");

    // Always add basic compilation test
    lib_content.push_str("    #[test]\n");
    lib_content.push_str("    fn test_compilation() {\n");
    lib_content.push_str(&format!(
        "        // Generated types compile for test: {}\n",
        test.name
    ));
    lib_content.push_str("    }\n\n");

    // Generate tests from scenarios
    for scenario in &test.test_scenarios {
        lib_content.push_str(&generate_test_from_scenario(scenario));
        lib_content.push('\n');
    }

    lib_content.push_str("}\n");
    lib_content
}

/// Generate test code from a test scenario
fn generate_test_from_scenario(scenario: &TestScenario) -> String {
    let mut test_code = String::new();

    // Convert type names to valid Rust type names (remove underscores)
    let target_type = to_rust_type_name(&scenario.target_type);

    test_code.push_str(&format!("    #[test]\n    fn {}() {{\n", scenario.name));

    match &scenario.behavior {
        TestBehavior::RoundTrip { json, assertions } => {
            // Generate round-trip test
            test_code.push_str(&format!("        let json_str = r#\"{json}\"#;\n"));
            test_code.push_str(&format!(
                "        let parsed: {target_type} = serde_json::from_str(json_str).unwrap();\n"
            ));

            // Add assertions
            for (field, expected) in assertions {
                test_code.push_str(&format!(
                    "        assert_eq!(parsed.{field}, serde_json::json!({expected}));\n"
                ));
            }

            // Test serialization round-trip
            test_code
                .push_str("        let serialized = serde_json::to_string(&parsed).unwrap();\n");
            test_code.push_str(&format!(
                "        let round_trip: {target_type} = serde_json::from_str(&serialized).unwrap();\n"
            ));
            test_code.push_str("        assert_eq!(serde_json::to_value(parsed).unwrap(), serde_json::to_value(round_trip).unwrap());\n");
        }

        TestBehavior::Construction { fields, assertions } => {
            // Generate construction test
            test_code.push_str(&format!("        let instance = {target_type} {{\n"));

            for (field_name, field_value) in fields {
                test_code.push_str(&format!(
                    "            {}: {},\n",
                    field_name,
                    field_value_to_code(field_value)
                ));
            }

            test_code.push_str("        };\n\n");

            // Add assertions
            for assertion in assertions {
                match assertion {
                    ConstructionAssertion::JsonContains(text) => {
                        test_code.push_str(
                            "        let json = serde_json::to_string(&instance).unwrap();\n",
                        );
                        test_code
                            .push_str(&format!("        assert!(json.contains(\"{text}\"));\n"));
                    }
                    ConstructionAssertion::FieldEquals(field, value) => {
                        test_code.push_str(&format!(
                            "        assert_eq!(instance.{field}, serde_json::json!({value}));\n"
                        ));
                    }
                    ConstructionAssertion::CanSerialize => {
                        test_code.push_str(
                            "        let _json = serde_json::to_string(&instance).unwrap();\n",
                        );
                    }
                }
            }
        }

        TestBehavior::CompileOnly => {
            test_code.push_str(&format!(
                "        // Type {target_type} exists and compiles\n"
            ));
            test_code.push_str(&format!("        let _: Option<{target_type}> = None;\n"));
        }

        TestBehavior::EnumMatch {
            json,
            expected_variant,
            variant_assertions,
        } => {
            test_code.push_str(&format!("        let json_str = r#\"{json}\"#;\n"));
            test_code.push_str(&format!(
                "        let parsed: {target_type} = serde_json::from_str(json_str).unwrap();\n"
            ));
            test_code.push_str("        match parsed {\n");

            // Generate match pattern for struct variants with named fields
            if variant_assertions.is_empty() {
                // No assertions needed, just check the variant
                test_code.push_str(&format!(
                    "            {target_type}::{expected_variant} {{ .. }} => {{\n"
                ));
                test_code.push_str("                // Variant matched successfully\n");
            } else {
                // Has assertions - generate struct variant pattern with field bindings
                let field_bindings: Vec<String> = variant_assertions
                    .iter()
                    .map(|(field, _)| field.clone())
                    .collect();

                if field_bindings.is_empty() {
                    test_code.push_str(&format!(
                        "            {target_type}::{expected_variant} {{ .. }} => {{\n"
                    ));
                } else {
                    // Always add .. to allow partial matching
                    let bindings = field_bindings.join(", ");
                    test_code.push_str(&format!(
                        "            {target_type}::{expected_variant} {{ {bindings}, .. }} => {{\n"
                    ));

                    // Add assertions on the fields
                    for (field, expected) in variant_assertions {
                        test_code.push_str(&format!(
                            "                assert_eq!({field}, serde_json::json!({expected}));\n"
                        ));
                    }
                }
            }

            test_code.push_str("            }\n");
            test_code.push_str(&format!(
                "            _ => panic!(\"Expected {expected_variant} variant\"),\n"
            ));
            test_code.push_str("        }\n");
        }
    }

    test_code.push_str("    }\n");
    test_code
}

/// Convert a FieldValue to Rust code
fn field_value_to_code(value: &FieldValue) -> String {
    match value {
        FieldValue::String(s) => format!("\"{s}\".to_string()"),
        FieldValue::Integer(i) => i.to_string(),
        FieldValue::Float(f) => f.to_string(),
        FieldValue::Boolean(b) => b.to_string(),
        FieldValue::Null => "None".to_string(),
        FieldValue::EnumVariant(variant) => variant.clone(),
        FieldValue::Array(values) => {
            let items: Vec<String> = values.iter().map(field_value_to_code).collect();
            format!("vec![{}]", items.join(", "))
        }
        FieldValue::Struct(type_name, fields) => {
            let mut code = format!("{type_name} {{\n");
            for (field_name, field_value) in fields {
                code.push_str(&format!(
                    "                {}: {},\n",
                    field_name,
                    field_value_to_code(field_value)
                ));
            }
            code.push_str("            }");
            code
        }
    }
}

/// Helper to create a minimal OpenAPI spec for testing
pub fn minimal_spec(schemas: Value) -> Value {
    serde_json::json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": schemas
        }
    })
}

/// Assert that the test compiled successfully
pub fn assert_compilation_success(result: &GenerationTestResult) {
    if let Some(output) = &result.compilation_output {
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            panic!("Compilation failed!\nSTDERR:\n{stderr}\nSTDOUT:\n{stdout}");
        }
    } else {
        panic!("No compilation was run");
    }
}

/// Find a type definition in the generated files
pub fn find_type_definition(result: &GenerationTestResult, type_name: &str) -> Option<String> {
    for file in &result.files {
        for line in file.content.lines() {
            if (line.contains(&format!("struct {type_name}"))
                || line.contains(&format!("enum {type_name}"))
                || line.contains(&format!("type {type_name} =")))
                && !line.trim().starts_with("//")
            {
                return Some(line.to_string());
            }
        }
    }
    None
}

/// Assert that a type name doesn't contain underscores
pub fn assert_no_underscores_in_type_name(type_definition: &str) {
    if let Some(type_name) = extract_type_name(type_definition) {
        assert!(
            !type_name.contains('_'),
            "Type name '{type_name}' should not contain underscores"
        );
    }
}

fn extract_type_name(type_def: &str) -> Option<String> {
    let parts: Vec<&str> = type_def.split_whitespace().collect();
    if parts.len() >= 2 {
        let name = parts[1].split('<').next()?.split('=').next()?.trim();
        Some(name.to_string())
    } else {
        None
    }
}

/// Convert a type name to valid Rust type name (PascalCase without underscores)
fn to_rust_type_name(s: &str) -> String {
    let mut result = String::new();
    let mut next_upper = true;

    for c in s.chars() {
        match c {
            'a'..='z' => {
                if next_upper {
                    result.push(c.to_ascii_uppercase());
                    next_upper = false;
                } else {
                    result.push(c);
                }
            }
            'A'..='Z' => {
                result.push(c);
                next_upper = false;
            }
            '0'..='9' => {
                result.push(c);
                next_upper = false;
            }
            '_' | '-' | '.' | ' ' => {
                // Skip underscore/separator and make next char uppercase
                next_upper = true;
            }
            _ => {
                // Other special characters - treat as word boundary
                next_upper = true;
            }
        }
    }

    result
}
