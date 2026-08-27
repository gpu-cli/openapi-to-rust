//! Synthetic JSON round-trip planning for generated Rust models.
//!
//! This is an internal conformance tool rather than a public data-faking API.
//! It deliberately uses the source schemas as the oracle: candidates are
//! generated deterministically, rejected unless an independent `jsonschema`
//! validator accepts them, then emitted into a scratch-crate test that runs
//! the exact generated Rust model through Serde twice.

use crate::{SchemaAnalyzer, analysis::component_schema_name_aliases, generator::rust_type_name};
use serde_json::{Map, Number, Value, json};
use std::collections::BTreeSet;

const MAX_ATTEMPTS_PER_SCHEMA: usize = 96;
const MAX_SYNTHESIS_DEPTH: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Draft4,
    Draft202012,
}

impl Dialect {
    fn from_spec(spec: &Value) -> Result<Self, Box<dyn std::error::Error>> {
        let version = spec
            .get("openapi")
            .and_then(Value::as_str)
            .ok_or("OpenAPI document has no string `openapi` version")?;
        if version.starts_with("3.0.") {
            Ok(Self::Draft4)
        } else if version.starts_with("3.1.") || version.starts_with("3.2.") {
            Ok(Self::Draft202012)
        } else {
            Err(format!("unsupported OpenAPI version `{version}`").into())
        }
    }

    fn definitions_key(self) -> &'static str {
        match self {
            Self::Draft4 => "definitions",
            Self::Draft202012 => "$defs",
        }
    }

    fn schema_uri(self) -> &'static str {
        match self {
            Self::Draft4 => "http://json-schema.org/draft-04/schema#",
            Self::Draft202012 => "https://json-schema.org/draft/2020-12/schema",
        }
    }

    fn rust_variant(self) -> &'static str {
        match self {
            Self::Draft4 => "jsonschema::Draft::Draft4",
            Self::Draft202012 => "jsonschema::Draft::Draft202012",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoundTripStats {
    pub component_schemas: usize,
    pub tested_schemas: usize,
    pub skipped_schemas: usize,
    pub samples: usize,
}

impl RoundTripStats {
    pub fn to_shell(&self) -> String {
        format!(
            "component_schemas={}\ntested_schemas={}\nskipped_schemas={}\nsamples={}\n",
            self.component_schemas, self.tested_schemas, self.skipped_schemas, self.samples
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedSchema {
    pub schema: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct RoundTripPlan {
    pub source: String,
    pub stats: RoundTripStats,
    pub skipped: Vec<SkippedSchema>,
}

#[derive(Debug)]
struct ModelCase {
    schema_name: String,
    rust_type: String,
    pointer: String,
    samples: Vec<Value>,
}

/// Build the Rust integration test mounted into a generated scratch crate.
pub fn build_round_trip_plan(
    spec: &Value,
    samples_per_schema: usize,
) -> Result<RoundTripPlan, Box<dyn std::error::Error>> {
    if samples_per_schema == 0 {
        return Err("samples_per_schema must be positive".into());
    }
    let dialect = Dialect::from_spec(spec)?;
    let raw_components = spec
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let component_aliases = component_schema_name_aliases(spec);

    let analysis = SchemaAnalyzer::new(spec.clone())?.analyze()?;
    let mut normalized = Map::new();
    for (name, schema) in &raw_components {
        normalized.insert(name.clone(), normalize_schema(schema, dialect));
    }
    let document = json!({
        "$schema": dialect.schema_uri(),
        dialect.definitions_key(): Value::Object(normalized),
    });

    let validators = validator_options(dialect).build_map(&document)?;
    let generator = SyntheticGenerator { root: &document };
    let mut cases = Vec::new();
    let mut skipped = Vec::new();

    for (name, raw_schema) in &raw_components {
        let analyzed_name = component_aliases.get(name).unwrap_or(name);
        let Some(model) = analysis.schemas.get(analyzed_name) else {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: "analysis did not emit a named Rust model".to_string(),
            });
            continue;
        };
        if contains_required_binary_format(raw_schema, &document, 0) {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: "required format: binary is a raw-body contract, not JSON".to_string(),
            });
            continue;
        }

        let pointer = format!(
            "#/{}/{}",
            dialect.definitions_key(),
            escape_pointer_segment(name)
        );
        let Some(validator) = validators.get(&pointer) else {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: format!("validator map did not retain target `{pointer}`"),
            });
            continue;
        };
        let target = document
            .pointer(pointer.trim_start_matches('#'))
            .ok_or_else(|| format!("missing normalized schema at {pointer}"))?;
        let mut samples = Vec::new();
        let mut seen = BTreeSet::new();
        for seed in 0..MAX_ATTEMPTS_PER_SCHEMA {
            let Some(candidate) = generator.generate(target, seed, 0, &mut Vec::new()) else {
                continue;
            };
            // A named Rust model is the non-null carrier for a nullable
            // component. Null is represented by Option at each reference
            // site, where enclosing model samples exercise it.
            if model.nullable && candidate.is_null() {
                continue;
            }
            if !validator.is_valid(&candidate) {
                continue;
            }
            let canonical = serde_json::to_string(&candidate)?;
            if seen.insert(canonical) {
                samples.push(candidate);
            }
            if samples.len() == samples_per_schema {
                break;
            }
        }
        if samples.is_empty() {
            let reason = if raw_schema == &Value::Bool(false) {
                "schema is uninhabited (`false`)".to_string()
            } else {
                "deterministic generator found no independently valid JSON instance".to_string()
            };
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason,
            });
            continue;
        }
        cases.push(ModelCase {
            schema_name: name.clone(),
            rust_type: rust_type_name(analyzed_name),
            pointer,
            samples,
        });
    }

    let stats = RoundTripStats {
        component_schemas: raw_components.len(),
        tested_schemas: cases.len(),
        skipped_schemas: skipped.len(),
        samples: cases.iter().map(|case| case.samples.len()).sum(),
    };
    let source = render_test_source(&document, dialect, &cases)?;
    Ok(RoundTripPlan {
        source,
        stats,
        skipped,
    })
}

fn validator_options(dialect: Dialect) -> jsonschema::ValidationOptions<'static> {
    jsonschema::options()
        .with_draft(match dialect {
            Dialect::Draft4 => jsonschema::Draft::Draft4,
            Dialect::Draft202012 => jsonschema::Draft::Draft202012,
        })
        .should_validate_formats(true)
        .with_pattern_options(jsonschema::PatternOptions::regex())
}

fn render_test_source(
    document: &Value,
    dialect: Dialect,
    cases: &[ModelCase],
) -> Result<String, serde_json::Error> {
    let document_literal = format!("{:?}", serde_json::to_string(document)?);
    let mut calls = String::new();
    for case in cases {
        let schema_name = format!("{:?}", case.schema_name);
        let pointer = format!("{:?}", case.pointer);
        let samples = format!("{:?}", serde_json::to_string(&case.samples)?);
        calls.push_str(&format!(
            "    failures.extend(check_model::<crate::generated::types::{}>(&validators, {}, {}, {}));\n",
            case.rust_type, schema_name, pointer, samples
        ));
    }

    Ok(format!(
        r#"//! Generated by the schema-roundtrip conformance tool. Do not edit.

use serde::{{Serialize, de::DeserializeOwned}};
use serde_json::Value;

fn errors(validator: &jsonschema::Validator, value: &Value) -> String {{
    validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}}

fn check_model<T>(
    validators: &jsonschema::ValidatorMap,
    schema_name: &str,
    pointer: &str,
    samples_json: &str,
) -> Vec<String>
where
    T: DeserializeOwned + Serialize,
{{
    let mut failures = Vec::new();
    let Some(validator) = validators.get(pointer) else {{
        failures.push(format!("{{schema_name}}: missing validator {{pointer}}"));
        return failures;
    }};
    let samples: Vec<Value> = match serde_json::from_str(samples_json) {{
        Ok(samples) => samples,
        Err(error) => {{
            failures.push(format!("{{schema_name}}: invalid embedded samples: {{error}}"));
            return failures;
        }}
    }};
    for (sample_index, input) in samples.into_iter().enumerate() {{
        if !validator.is_valid(&input) {{
            failures.push(format!(
                "{{schema_name}} sample {{sample_index}} synthetic precondition failed: {{}}; input={{input}}",
                errors(validator, &input),
            ));
            continue;
        }}
        let hydrated: T = match serde_json::from_value(input.clone()) {{
            Ok(hydrated) => hydrated,
            Err(error) => {{
                failures.push(format!(
                    "{{schema_name}} sample {{sample_index}} failed Rust hydration: {{error}}; input={{input}}"
                ));
                continue;
            }}
        }};
        let output = match serde_json::to_value(&hydrated) {{
            Ok(output) => output,
            Err(error) => {{
                failures.push(format!(
                    "{{schema_name}} sample {{sample_index}} failed Rust serialization: {{error}}; input={{input}}"
                ));
                continue;
            }}
        }};
        if !validator.is_valid(&output) {{
            failures.push(format!(
                "{{schema_name}} sample {{sample_index}} emitted schema-invalid JSON: {{}}; input={{input}}; output={{output}}",
                errors(validator, &output),
            ));
        }}
        let hydrated_again: T = match serde_json::from_value(output.clone()) {{
            Ok(hydrated) => hydrated,
            Err(error) => {{
                failures.push(format!(
                    "{{schema_name}} sample {{sample_index}} output could not hydrate again: {{error}}; input={{input}}; output={{output}}"
                ));
                continue;
            }}
        }};
        let stable = match serde_json::to_value(&hydrated_again) {{
            Ok(stable) => stable,
            Err(error) => {{
                failures.push(format!(
                    "{{schema_name}} sample {{sample_index}} second serialization failed: {{error}}; input={{input}}; output={{output}}"
                ));
                continue;
            }}
        }};
        if output != stable {{
            failures.push(format!(
                "{{schema_name}} sample {{sample_index}} did not reach a stable wire representation; input={{input}}; output={{output}}; stable={{stable}}"
            ));
        }}
    }}
    failures
}}

#[test]
fn generated_models_preserve_schema_valid_json() {{
    let document: Value = serde_json::from_str({document_literal})
        .unwrap_or_else(|error| panic!("invalid embedded schema bundle: {{error}}"));
    let validators = jsonschema::options()
        .with_draft({draft})
        .should_validate_formats(true)
        .with_pattern_options(jsonschema::PatternOptions::regex())
        .build_map(&document)
        .unwrap_or_else(|error| panic!("schema bundle did not compile: {{error}}"));
    let mut failures: Vec<String> = Vec::new();
{calls}    if !failures.is_empty() {{
        panic!(
            "{{}} schema round-trip failure(s):\n\n{{}}",
            failures.len(),
            failures.join("\n\n"),
        );
    }}
}}
"#,
        draft = dialect.rust_variant(),
    ))
}

fn normalize_schema(value: &Value, dialect: Dialect) -> Value {
    let Value::Object(source) = value else {
        return value.clone();
    };
    let mut schema = source.clone();
    for child in schema.values_mut() {
        normalize_schema_children(child, dialect);
    }
    if let Some(Value::String(reference)) = schema.get_mut("$ref") {
        let prefix = "#/components/schemas/";
        if let Some(name) = reference.strip_prefix(prefix) {
            *reference = format!("#/{}/{name}", dialect.definitions_key());
        }
    }
    if dialect == Dialect::Draft4
        && schema.remove("nullable").and_then(|value| value.as_bool()) == Some(true)
    {
        return json!({ "anyOf": [Value::Object(schema), { "type": "null" }] });
    }
    Value::Object(schema)
}

fn normalize_schema_children(value: &mut Value, dialect: Dialect) {
    match value {
        Value::Array(values) => {
            for value in values {
                *value = normalize_schema(value, dialect);
            }
        }
        Value::Object(_) => *value = normalize_schema(value, dialect),
        _ => {}
    }
}

fn escape_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn contains_required_binary_format(schema: &Value, root: &Value, depth: usize) -> bool {
    if depth > MAX_SYNTHESIS_DEPTH {
        return false;
    }
    let Value::Object(object) = schema else {
        return false;
    };
    if object.get("format").and_then(Value::as_str) == Some("binary") {
        return true;
    }
    if let Some(reference) = object.get("$ref").and_then(Value::as_str)
        && let Some(target) = resolve_local_ref(root, reference)
    {
        return contains_required_binary_format(target, root, depth + 1);
    }
    if let Some(Value::Array(branches)) = object.get("allOf")
        && branches
            .iter()
            .any(|branch| contains_required_binary_format(branch, root, depth + 1))
    {
        return true;
    }
    let required: BTreeSet<&str> = object
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    object
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|properties| {
            properties.iter().any(|(name, child)| {
                required.contains(name.as_str())
                    && contains_required_binary_format(child, root, depth + 1)
            })
        })
}

struct SyntheticGenerator<'a> {
    root: &'a Value,
}

impl SyntheticGenerator<'_> {
    fn generate(
        &self,
        schema: &Value,
        seed: usize,
        depth: usize,
        refs: &mut Vec<String>,
    ) -> Option<Value> {
        if depth > MAX_SYNTHESIS_DEPTH {
            return None;
        }
        match schema {
            Value::Bool(true) => return Some(any_value(seed, depth)),
            Value::Bool(false) => return None,
            Value::Object(_) => {}
            _ => return Some(any_value(seed, depth)),
        }
        let object = schema.as_object()?;

        if seed == 0
            && let Some(default) = object.get("default")
        {
            return Some(default.clone());
        }
        if seed == 1
            && let Some(example) = object
                .get("examples")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .or_else(|| object.get("example"))
        {
            return Some(example.clone());
        }
        if let Some(constant) = object.get("const") {
            return Some(constant.clone());
        }
        if let Some(values) = object.get("enum").and_then(Value::as_array) {
            if values.is_empty() {
                return None;
            }
            return values.get(seed % values.len()).cloned();
        }
        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            if refs.iter().any(|seen| seen == reference) {
                return None;
            }
            let target = resolve_local_ref(self.root, reference)?;
            refs.push(reference.to_string());
            let generated = self.generate(target, seed + 2, depth + 1, refs);
            refs.pop();
            return generated;
        }
        if let Some(branches) = object.get("oneOf").and_then(Value::as_array) {
            return self.generate_branch(branches, seed, depth, refs);
        }
        if let Some(branches) = object.get("anyOf").and_then(Value::as_array) {
            return self.generate_branch(branches, seed, depth, refs);
        }
        if let Some(branches) = object.get("allOf").and_then(Value::as_array) {
            let mut merged = Value::Null;
            for (index, branch) in branches.iter().enumerate() {
                let part = self.generate(branch, seed + index + 2, depth + 1, refs)?;
                merged = merge_values(merged, part)?;
            }
            return Some(merged);
        }

        let types = schema_types(object);
        let selected = types.get(seed % types.len().max(1)).copied();
        match selected {
            Some("null") => Some(Value::Null),
            Some("boolean") => Some(Value::Bool(seed.is_multiple_of(2))),
            Some("integer") => numeric_value(object, seed, true),
            Some("number") => numeric_value(object, seed, false),
            Some("string") => Some(Value::String(string_value(object, seed))),
            Some("array") => self.array_value(object, seed, depth, refs),
            Some("object") => self.object_value(object, seed, depth, refs),
            _ => Some(any_value(seed, depth)),
        }
    }

    fn generate_branch(
        &self,
        branches: &[Value],
        seed: usize,
        depth: usize,
        refs: &mut Vec<String>,
    ) -> Option<Value> {
        if branches.is_empty() {
            return None;
        }
        for offset in 0..branches.len() {
            let index = (seed + offset) % branches.len();
            if let Some(value) = self.generate(&branches[index], seed + offset + 2, depth + 1, refs)
            {
                return Some(value);
            }
        }
        None
    }

    fn object_value(
        &self,
        object: &Map<String, Value>,
        seed: usize,
        depth: usize,
        refs: &mut Vec<String>,
    ) -> Option<Value> {
        let properties = object
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let required: BTreeSet<String> = object
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let mut output = Map::new();
        for (index, name) in required.iter().enumerate() {
            let child = properties
                .get(name)
                .or_else(|| object.get("additionalProperties"))
                .unwrap_or(&Value::Bool(true));
            let value = self.generate(child, seed + index + 2, depth + 1, refs)?;
            output.insert(name.clone(), value);
        }

        let optional: Vec<_> = properties
            .iter()
            .filter(|(name, _)| !required.contains(*name))
            .collect();
        let optional_count = if optional.is_empty() {
            0
        } else {
            seed % (optional.len() + 1)
        };
        for (index, (name, child)) in optional.into_iter().take(optional_count).enumerate() {
            if let Some(value) = self.generate(child, seed + index + 7, depth + 1, refs) {
                output.insert(name.clone(), value);
            }
        }

        if seed % 4 == 3
            && let Some(additional) = object.get("additionalProperties")
            && additional != &Value::Bool(false)
            && let Some(value) = self.generate(additional, seed + 13, depth + 1, refs)
        {
            output.insert(format!("synthetic_extra_{seed}"), value);
        }
        apply_dependent_required(self, object, &properties, seed, depth, refs, &mut output)?;

        let min_properties = object
            .get("minProperties")
            .and_then(json_count)
            .unwrap_or(0) as usize;
        if output.len() < min_properties {
            return None;
        }
        Some(Value::Object(output))
    }

    fn array_value(
        &self,
        object: &Map<String, Value>,
        seed: usize,
        depth: usize,
        refs: &mut Vec<String>,
    ) -> Option<Value> {
        let min = object.get("minItems").and_then(json_count).unwrap_or(0) as usize;
        let max = object
            .get("maxItems")
            .and_then(json_count)
            .map(|value| value as usize)
            .unwrap_or(min.saturating_add(4));
        if min > max {
            return None;
        }
        let target_len = min.saturating_add(seed % 3).min(max);
        let prefix = object
            .get("prefixItems")
            .and_then(Value::as_array)
            .or_else(|| object.get("items").and_then(Value::as_array));
        let items = match object.get("items") {
            Some(Value::Array(_)) => None,
            other => other,
        };
        let mut output = Vec::new();
        for index in 0..target_len {
            let child = prefix
                .and_then(|schemas| schemas.get(index))
                .or(items)
                .unwrap_or(&Value::Bool(true));
            if child == &Value::Bool(false) {
                return None;
            }
            let mut value = self.generate(child, seed + index + 2, depth + 1, refs)?;
            if object.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
                for retry in 0..8 {
                    if !output.contains(&value) {
                        break;
                    }
                    value = self.generate(child, seed + index + retry + 17, depth + 1, refs)?;
                }
                if output.contains(&value) {
                    return None;
                }
            }
            output.push(value);
        }
        if let Some(contains) = object.get("contains") {
            let needed = object.get("minContains").and_then(json_count).unwrap_or(1) as usize;
            while output.len() < needed && output.len() < max {
                output.push(self.generate(contains, seed + output.len() + 29, depth + 1, refs)?);
            }
            for index in 0..needed.min(output.len()) {
                output[index] = self.generate(contains, seed + index + 31, depth + 1, refs)?;
            }
        }
        Some(Value::Array(output))
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_dependent_required(
    generator: &SyntheticGenerator<'_>,
    object: &Map<String, Value>,
    properties: &Map<String, Value>,
    seed: usize,
    depth: usize,
    refs: &mut Vec<String>,
    output: &mut Map<String, Value>,
) -> Option<()> {
    let dependencies = object
        .get("dependentRequired")
        .or_else(|| object.get("dependencies"))
        .and_then(Value::as_object);
    let Some(dependencies) = dependencies else {
        return Some(());
    };
    let triggers: Vec<String> = output.keys().cloned().collect();
    for trigger in triggers {
        let Some(names) = dependencies.get(&trigger).and_then(Value::as_array) else {
            continue;
        };
        for (index, name) in names.iter().filter_map(Value::as_str).enumerate() {
            if output.contains_key(name) {
                continue;
            }
            let child = properties.get(name).unwrap_or(&Value::Bool(true));
            let value = generator.generate(child, seed + index + 41, depth + 1, refs)?;
            output.insert(name.to_string(), value);
        }
    }
    Some(())
}

fn schema_types(object: &Map<String, Value>) -> Vec<&str> {
    match object.get("type") {
        Some(Value::String(value)) => return vec![value.as_str()],
        Some(Value::Array(values)) => {
            let types: Vec<_> = values.iter().filter_map(Value::as_str).collect();
            if !types.is_empty() {
                return types;
            }
        }
        _ => {}
    }
    if object.contains_key("properties")
        || object.contains_key("required")
        || object.contains_key("additionalProperties")
        || object.contains_key("minProperties")
    {
        vec!["object"]
    } else if object.contains_key("items")
        || object.contains_key("prefixItems")
        || object.contains_key("minItems")
    {
        vec!["array"]
    } else {
        vec![
            "null", "boolean", "integer", "number", "string", "array", "object",
        ]
    }
}

fn numeric_value(object: &Map<String, Value>, seed: usize, integer: bool) -> Option<Value> {
    let minimum = object.get("minimum").and_then(Value::as_f64).unwrap_or(0.0);
    let maximum = object
        .get("maximum")
        .and_then(Value::as_f64)
        .unwrap_or(minimum + 100.0);
    let mut value = match seed % 3 {
        0 => minimum,
        1 => maximum,
        _ => (minimum + maximum) / 2.0,
    };
    if let Some(exclusive) = object.get("exclusiveMinimum").and_then(Value::as_f64) {
        value = value.max(exclusive + if integer { 1.0 } else { 0.5 });
    } else if object.get("exclusiveMinimum").and_then(Value::as_bool) == Some(true) {
        value = value.max(minimum + if integer { 1.0 } else { 0.5 });
    }
    if let Some(exclusive) = object.get("exclusiveMaximum").and_then(Value::as_f64) {
        value = value.min(exclusive - if integer { 1.0 } else { 0.5 });
    } else if object.get("exclusiveMaximum").and_then(Value::as_bool) == Some(true) {
        value = value.min(maximum - if integer { 1.0 } else { 0.5 });
    }
    if let Some(multiple) = object.get("multipleOf").and_then(Value::as_f64)
        && multiple > 0.0
    {
        value = (value / multiple).ceil() * multiple;
    }
    if integer {
        let integer_value = value.round();
        if integer_value >= 0.0 && integer_value <= u64::MAX as f64 {
            return Some(Value::Number(Number::from(integer_value as u64)));
        }
        if integer_value >= i64::MIN as f64 && integer_value <= i64::MAX as f64 {
            return Some(Value::Number(Number::from(integer_value as i64)));
        }
        None
    } else {
        Number::from_f64(value).map(Value::Number)
    }
}

fn string_value(object: &Map<String, Value>, seed: usize) -> String {
    let min = object.get("minLength").and_then(json_count).unwrap_or(0) as usize;
    let max = object
        .get("maxLength")
        .and_then(json_count)
        .map(|value| value as usize)
        .unwrap_or(usize::MAX);
    let format = object.get("format").and_then(Value::as_str);
    let mut value = match format {
        Some("date-time") => "2024-01-02T03:04:05Z".to_string(),
        Some("date") => "2024-01-02".to_string(),
        Some("time") => "03:04:05Z".to_string(),
        Some("duration") => "P1DT2H".to_string(),
        Some("uuid") => "123e4567-e89b-12d3-a456-426614174000".to_string(),
        Some("uri") | Some("url") => "https://example.com/resource".to_string(),
        Some("uri-reference") => "/resource/1".to_string(),
        Some("email") => "agent@example.com".to_string(),
        Some("hostname") => "example.com".to_string(),
        Some("ipv4") => "192.0.2.1".to_string(),
        Some("ipv6") => "2001:db8::1".to_string(),
        Some("byte") => "AQID".to_string(),
        Some("json-pointer") => "/items/0".to_string(),
        _ => object
            .get("pattern")
            .and_then(Value::as_str)
            .map(|pattern| pattern_string(pattern, min, seed))
            .unwrap_or_else(|| format!("synthetic{seed}")),
    };
    while value.chars().count() < min {
        value.push('x');
    }
    if value.chars().count() > max {
        value = value.chars().take(max).collect();
    }
    value
}

fn pattern_string(pattern: &str, min: usize, seed: usize) -> String {
    let fill = if pattern.contains("[A-Z]") {
        'A'
    } else if pattern.contains("[0-9]") || pattern.contains("\\d") {
        '1'
    } else if pattern.contains("[a-zA-Z]") {
        'a'
    } else {
        'x'
    };
    let quantified_min = pattern
        .split('{')
        .nth(1)
        .and_then(|tail| tail.split([',', '}']).next())
        .and_then(|number| number.parse::<usize>().ok())
        .unwrap_or_else(|| usize::from(pattern.contains('+')));
    let len = min.max(quantified_min).max(1).saturating_add(seed % 2);
    std::iter::repeat_n(fill, len).collect()
}

fn json_count(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|v| v.fract() == 0.0 && *v >= 0.0)
            .map(|v| v as u64)
    })
}

fn any_value(seed: usize, depth: usize) -> Value {
    match seed % 7 {
        0 => Value::Null,
        1 => Value::Bool(seed.is_multiple_of(2)),
        2 => json!(seed as i64),
        3 => json!(seed as f64 + 0.5),
        4 => Value::String(format!("synthetic_{seed}")),
        5 if depth < MAX_SYNTHESIS_DEPTH => Value::Array(vec![json!(seed)]),
        6 if depth < MAX_SYNTHESIS_DEPTH => {
            Value::Object(Map::from_iter([("value".to_string(), json!(seed))]))
        }
        _ => Value::Null,
    }
}

fn merge_values(left: Value, right: Value) -> Option<Value> {
    match (left, right) {
        (Value::Null, value) | (value, Value::Null) => Some(value),
        (Value::Object(mut left), Value::Object(right)) => {
            for (key, value) in right {
                if let Some(existing) = left.remove(&key) {
                    left.insert(key, merge_values(existing, value)?);
                } else {
                    left.insert(key, value);
                }
            }
            Some(Value::Object(left))
        }
        (left, right) if left == right => Some(left),
        _ => None,
    }
}

fn resolve_local_ref<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_varied_samples_and_compiled_test_source() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "round trip", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "State": { "type": "string", "enum": ["new", "done"] },
                "Item": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "state"],
                    "properties": {
                        "id": { "type": "string", "format": "uuid" },
                        "state": { "$ref": "#/components/schemas/State" },
                        "note": { "type": ["string", "null"], "minLength": 1 }
                    }
                }
            }}
        });
        let dialect = Dialect::from_spec(&spec).expect("dialect");
        let raw = spec
            .pointer("/components/schemas")
            .and_then(Value::as_object)
            .expect("components");
        let normalized = raw
            .iter()
            .map(|(name, schema)| (name.clone(), normalize_schema(schema, dialect)))
            .collect::<Map<_, _>>();
        let document = json!({
            "$schema": dialect.schema_uri(),
            "$defs": Value::Object(normalized),
        });
        let validators = validator_options(dialect)
            .build_map(&document)
            .expect("validators");
        let validator = validators.get("#/$defs/Item").expect("item validator");
        let target = document.pointer("/$defs/Item").expect("item target");
        let candidate = SyntheticGenerator { root: &document }
            .generate(target, 0, 0, &mut Vec::new())
            .expect("item candidate");
        assert!(
            validator.is_valid(&candidate),
            "candidate {candidate}; errors: {:?}",
            validator
                .iter_errors(&candidate)
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
        );

        let plan = build_round_trip_plan(&spec, 4).expect("plan");
        assert_eq!(plan.stats.component_schemas, 2);
        assert_eq!(plan.stats.tested_schemas, 2, "skipped: {:?}", plan.skipped);
        assert!(plan.stats.samples >= 4);
        assert!(plan.source.contains("crate::generated::types::Item"));
        assert!(plan.source.contains("crate::generated::types::State"));
        assert!(plan.source.contains("validator.is_valid(&output)"));
    }

    #[test]
    fn rendered_test_collects_every_model_failure_before_panicking() {
        let document = json!({
            "$schema": Dialect::Draft202012.schema_uri(),
            "$defs": {
                "First": { "type": "string" },
                "Second": { "type": "integer" }
            }
        });
        let cases = vec![
            ModelCase {
                schema_name: "First".to_string(),
                rust_type: "First".to_string(),
                pointer: "#/$defs/First".to_string(),
                samples: vec![json!("one"), json!("two")],
            },
            ModelCase {
                schema_name: "Second".to_string(),
                rust_type: "Second".to_string(),
                pointer: "#/$defs/Second".to_string(),
                samples: vec![json!(1), json!(2)],
            },
        ];

        let source = render_test_source(&document, Dialect::Draft202012, &cases)
            .expect("rendered test source");
        let model_check = source.split("#[test]").next().expect("model check helper");
        assert!(model_check.contains("fn check_model<T>("));
        assert!(model_check.contains(") -> Vec<String>"));
        assert!(model_check.contains("missing validator {pointer}"));
        assert!(model_check.contains("invalid embedded samples: {error}"));
        assert!(!model_check.contains("panic!("));
        assert!(!model_check.contains("assert!("));
        assert!(!model_check.contains("assert_eq!("));

        let first_call = source
            .find("failures.extend(check_model::<crate::generated::types::First>")
            .expect("first model call");
        let second_call = source
            .find("failures.extend(check_model::<crate::generated::types::Second>")
            .expect("second model call");
        let aggregate_panic = source
            .rfind("if !failures.is_empty()")
            .expect("aggregate failure guard");
        assert!(first_call < second_call);
        assert!(second_call < aggregate_panic);
        assert!(source[aggregate_panic..].contains("failures.join(\"\\n\\n\")"));
        assert!(!source.contains("assert_model"));
    }

    #[test]
    fn classifies_false_schema_as_uninhabited() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "round trip", "version": "1" },
            "paths": {},
            "components": { "schemas": { "Never": false } }
        });
        let plan = build_round_trip_plan(&spec, 2).expect("plan");
        assert_eq!(plan.stats.tested_schemas, 0);
        assert_eq!(plan.stats.skipped_schemas, 1);
        assert!(plan.skipped[0].reason.contains("uninhabited"));
    }

    #[test]
    fn uses_emitted_names_for_colliding_component_keys() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "round trip", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "ItemStatus": { "type": "string", "enum": ["ready"] },
                "item.status": { "type": "string", "enum": ["waiting"] }
            }}
        });

        let plan = build_round_trip_plan(&spec, 2).expect("plan");
        assert_eq!(plan.stats.tested_schemas, 2, "skipped: {:?}", plan.skipped);
        assert!(plan.source.contains("crate::generated::types::ItemStatus"));
        assert!(plan.source.contains("crate::generated::types::ItemStatus2"));
    }
}
