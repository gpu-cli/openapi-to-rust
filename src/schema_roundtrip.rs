//! Synthetic JSON round-trip planning for generated Rust models.
//!
//! This is an internal conformance tool rather than a public data-faking API.
//! It deliberately uses the source schemas as the oracle: candidates are
//! generated deterministically, rejected unless an independent `jsonschema`
//! validator accepts them, then emitted into a scratch-crate test that runs
//! the exact generated Rust model through Serde twice.

use crate::{
    SchemaAnalyzer, analysis::component_schema_name_aliases, generator::rust_type_name,
    spec_source::parse_oas_version, type_mapping::normalize_builtin_format,
};
use serde_json::{Map, Number, Value, json};
use std::collections::{BTreeMap, BTreeSet};

const MAX_ATTEMPTS_PER_SCHEMA: usize = 96;
const MAX_SYNTHESIS_DEPTH: usize = 20;
const MAX_DYNAMIC_REF_OCCURRENCES: usize = 2;

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
        match parse_oas_version(version) {
            Some((3, 0)) => Ok(Self::Draft4),
            Some((3, 1 | 2)) => Ok(Self::Draft202012),
            _ => Err(format!("unsupported OpenAPI version `{version}`").into()),
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
    pub source_invalid_schemas: usize,
    pub dependent_schemas: usize,
    pub synthesis_skipped_schemas: usize,
    pub samples: usize,
}

impl RoundTripStats {
    pub fn to_shell(&self) -> String {
        format!(
            concat!(
                "component_schemas={}\n",
                "tested_schemas={}\n",
                "skipped_schemas={}\n",
                "source_invalid_schemas={}\n",
                "dependent_schemas={}\n",
                "synthesis_skipped_schemas={}\n",
                "samples={}\n"
            ),
            self.component_schemas,
            self.tested_schemas,
            self.skipped_schemas,
            self.source_invalid_schemas,
            self.dependent_schemas,
            self.synthesis_skipped_schemas,
            self.samples
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

#[derive(Debug, Default)]
struct SchemaQuarantine {
    source_invalid: BTreeMap<String, String>,
    dependent: BTreeMap<String, String>,
}

impl SchemaQuarantine {
    fn contains(&self, name: &str) -> bool {
        self.source_invalid.contains_key(name) || self.dependent.contains_key(name)
    }
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
        normalized.insert(
            name.clone(),
            normalize_component_schema(name, schema, dialect),
        );
    }
    let quarantine = quarantine_invalid_components(&normalized, dialect);
    let valid_components = normalized
        .into_iter()
        .filter(|(name, _)| !quarantine.contains(name))
        .collect::<Map<_, _>>();
    let document = json!({
        "$schema": dialect.schema_uri(),
        dialect.definitions_key(): Value::Object(valid_components),
    });

    let validators = validator_options(dialect).build_map(&document)?;
    let generator = SyntheticGenerator::with_validators(&document, &validators);
    let mut cases = Vec::new();
    let mut skipped = Vec::new();

    for (name, raw_schema) in &raw_components {
        if let Some(reason) = quarantine.source_invalid.get(name) {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: reason.clone(),
            });
            continue;
        }
        if let Some(reason) = quarantine.dependent.get(name) {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: reason.clone(),
            });
            continue;
        }
        let pointer = format!(
            "#/{}/{}",
            dialect.definitions_key(),
            escape_pointer_segment(name)
        );
        let target = document
            .pointer(pointer.trim_start_matches('#'))
            .ok_or_else(|| format!("missing normalized schema at {pointer}"))?;
        let analyzed_name = component_aliases.get(name).unwrap_or(name);
        let Some(model) = analysis.schemas.get(analyzed_name) else {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: "analysis did not emit a named Rust model".to_string(),
            });
            continue;
        };
        if contains_required_binary_format(target, &document) {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: "required format: binary is a raw-body contract, not JSON".to_string(),
            });
            continue;
        }

        let Some(validator) = validators.get(&pointer) else {
            skipped.push(SkippedSchema {
                schema: name.clone(),
                reason: format!("validator map did not retain target `{pointer}`"),
            });
            continue;
        };
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
        source_invalid_schemas: quarantine.source_invalid.len(),
        dependent_schemas: quarantine.dependent.len(),
        synthesis_skipped_schemas: skipped
            .len()
            .saturating_sub(quarantine.source_invalid.len() + quarantine.dependent.len()),
        samples: cases.iter().map(|case| case.samples.len()).sum(),
    };
    debug_assert_eq!(
        stats.skipped_schemas,
        stats.source_invalid_schemas + stats.dependent_schemas + stats.synthesis_skipped_schemas
    );
    debug_assert_eq!(
        stats.component_schemas,
        stats.tested_schemas + stats.skipped_schemas
    );
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

fn quarantine_invalid_components(
    normalized: &Map<String, Value>,
    dialect: Dialect,
) -> SchemaQuarantine {
    let known: BTreeSet<&str> = normalized.keys().map(String::as_str).collect();
    let mut quarantine = SchemaQuarantine::default();
    let mut dependencies = BTreeMap::<String, BTreeSet<String>>::new();

    for (name, schema) in normalized {
        if let Some(reason) = legacy_recursive_scope_error(name, schema, dialect) {
            quarantine.source_invalid.insert(name.clone(), reason);
        }
        let mut isolated = schema.clone();
        neutralize_local_component_refs(&mut isolated, dialect);
        if let Some(reason) = meta_validation_error(name, &isolated, dialect) {
            quarantine
                .source_invalid
                .entry(name.clone())
                .or_insert(reason);
        }

        let mut refs = Vec::new();
        collect_local_component_refs(schema, dialect, "", &mut refs);
        let mut component_dependencies = BTreeSet::new();
        for local_ref in refs {
            if known.contains(local_ref.target.as_str()) {
                component_dependencies.insert(local_ref.target);
            } else {
                quarantine.source_invalid.entry(name.clone()).or_insert_with(|| {
                    format!(
                        "source schema invalid at #/components/schemas/{}{}: unresolved local component reference `{}`",
                        escape_pointer_segment(name), local_ref.location, local_ref.reference
                    )
                });
            }
        }
        dependencies.insert(name.clone(), component_dependencies);
    }

    let mut blocked: BTreeSet<String> = quarantine.source_invalid.keys().cloned().collect();
    loop {
        let mut added = Vec::new();
        for (name, referenced) in &dependencies {
            if blocked.contains(name) {
                continue;
            }
            if let Some(dependency) = referenced.iter().find(|target| blocked.contains(*target)) {
                added.push((name.clone(), dependency.clone()));
            }
        }
        if added.is_empty() {
            break;
        }
        for (name, dependency) in added {
            blocked.insert(name.clone());
            quarantine.dependent.insert(
                name,
                format!(
                    "depends on quarantined component at #/components/schemas/{}",
                    escape_pointer_segment(&dependency)
                ),
            );
        }
    }

    quarantine
}

fn meta_validation_error(name: &str, schema: &Value, dialect: Dialect) -> Option<String> {
    let error = match dialect {
        Dialect::Draft4 => jsonschema::draft4::meta::validate(schema).err(),
        Dialect::Draft202012 => jsonschema::draft202012::meta::validate(schema).err(),
    }?;
    Some(format!(
        "source schema invalid at #/components/schemas/{}{}: {} (meta-schema path {})",
        escape_pointer_segment(name),
        error.instance_path(),
        error,
        error.schema_path()
    ))
}

fn neutralize_local_component_refs(value: &mut Value, dialect: Dialect) {
    match value {
        Value::Array(values) => {
            for value in values {
                neutralize_local_component_refs(value, dialect);
            }
        }
        Value::Object(object) => {
            if object
                .get("$ref")
                .and_then(Value::as_str)
                .and_then(|reference| normalized_component_ref_target(reference, dialect))
                .is_some()
            {
                object.remove("$ref");
            }
            for value in object.values_mut() {
                neutralize_local_component_refs(value, dialect);
            }
        }
        _ => {}
    }
}

#[derive(Debug)]
struct LocalComponentRef {
    location: String,
    target: String,
    reference: String,
}

fn collect_local_component_refs(
    value: &Value,
    dialect: Dialect,
    location: &str,
    refs: &mut Vec<LocalComponentRef>,
) {
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_local_component_refs(value, dialect, &format!("{location}/{index}"), refs);
            }
        }
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str)
                && let Some(target) = normalized_component_ref_target(reference, dialect)
            {
                refs.push(LocalComponentRef {
                    location: format!("{location}/$ref"),
                    target,
                    reference: reference.to_string(),
                });
            }
            for (key, value) in object {
                collect_local_component_refs(
                    value,
                    dialect,
                    &format!("{location}/{}", escape_pointer_segment(key)),
                    refs,
                );
            }
        }
        _ => {}
    }
}

fn normalized_component_ref_target(reference: &str, dialect: Dialect) -> Option<String> {
    let prefix = format!("#/{}/", dialect.definitions_key());
    let segment = reference.strip_prefix(&prefix)?.split('/').next()?;
    unescape_pointer_segment(segment)
}

fn unescape_pointer_segment(value: &str) -> Option<String> {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            output.push(character);
            continue;
        }
        match chars.next()? {
            '0' => output.push('~'),
            '1' => output.push('/'),
            _ => return None,
        }
    }
    Some(output)
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

fn normalize_component_schema(name: &str, value: &Value, dialect: Dialect) -> Value {
    let mut normalized = normalize_schema(value, dialect);
    if dialect == Dialect::Draft202012 {
        migrate_safe_recursive_scope(name, &mut normalized);
    }
    normalized
}

#[derive(Debug)]
struct RecursiveScopeIssue {
    location: String,
    message: String,
}

#[derive(Debug, Default)]
struct LegacyRecursiveScopeAudit {
    has_legacy_keyword: bool,
    recursive_refs: usize,
    issue: Option<RecursiveScopeIssue>,
}

impl LegacyRecursiveScopeAudit {
    fn record_issue(&mut self, location: String, message: impl Into<String>) {
        if self.issue.is_none() {
            self.issue = Some(RecursiveScopeIssue {
                location,
                message: message.into(),
            });
        }
    }
}

fn migrate_safe_recursive_scope(component_name: &str, schema: &mut Value) {
    let audit = audit_legacy_recursive_scope(schema);
    if !audit.has_legacy_keyword || audit.issue.is_some() {
        return;
    }

    let anchor = component_dynamic_anchor(component_name);
    rewrite_recursive_scope(schema, &anchor, true);
}

fn audit_legacy_recursive_scope(schema: &Value) -> LegacyRecursiveScopeAudit {
    let mut audit = LegacyRecursiveScopeAudit::default();
    inspect_legacy_recursive_scope(schema, "", true, &mut audit);

    if audit.has_legacy_keyword {
        let root_anchor = schema
            .as_object()
            .and_then(|object| object.get("$recursiveAnchor"));
        if root_anchor != Some(&Value::Bool(true)) {
            audit.record_issue(
                "/$recursiveAnchor".to_string(),
                "migration requires `$recursiveAnchor: true` at the component root",
            );
        } else if audit.recursive_refs == 0 {
            audit.record_issue(
                "/$recursiveAnchor".to_string(),
                "component-root `$recursiveAnchor` has no descendant `$recursiveRef: \"#\"` pair",
            );
        }
    }
    audit
}

fn inspect_legacy_recursive_scope(
    value: &Value,
    location: &str,
    is_root: bool,
    audit: &mut LegacyRecursiveScopeAudit,
) {
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                inspect_legacy_recursive_scope(value, &format!("{location}/{index}"), false, audit);
            }
        }
        Value::Object(object) => {
            if let Some(anchor) = object.get("$recursiveAnchor") {
                audit.has_legacy_keyword = true;
                if !is_root {
                    audit.record_issue(
                        format!("{location}/$recursiveAnchor"),
                        "nested `$recursiveAnchor` changes the recursive scope",
                    );
                } else if anchor != &Value::Bool(true) {
                    audit.record_issue(
                        "/$recursiveAnchor".to_string(),
                        "component-root `$recursiveAnchor` must be `true`",
                    );
                }
            }
            if let Some(reference) = object.get("$recursiveRef") {
                audit.has_legacy_keyword = true;
                audit.recursive_refs += 1;
                if reference.as_str() != Some("#") {
                    audit.record_issue(
                        format!("{location}/$recursiveRef"),
                        "only `$recursiveRef: \"#\"` can be migrated safely",
                    );
                }
            }
            if !is_root && object.contains_key("$id") {
                audit.record_issue(
                    format!("{location}/$id"),
                    "nested `$id` creates a distinct resource scope",
                );
            }
            for keyword in ["$anchor", "$dynamicAnchor"] {
                if object.contains_key(keyword) {
                    audit.record_issue(
                        format!("{location}/{keyword}"),
                        format!("existing `{keyword}` conflicts with recursive-scope migration"),
                    );
                }
            }
            for (key, value) in object {
                inspect_legacy_recursive_scope(
                    value,
                    &format!("{location}/{}", escape_pointer_segment(key)),
                    false,
                    audit,
                );
            }
        }
        _ => {}
    }
}

fn rewrite_recursive_scope(value: &mut Value, anchor: &str, is_root: bool) {
    match value {
        Value::Array(values) => {
            for value in values {
                rewrite_recursive_scope(value, anchor, false);
            }
        }
        Value::Object(object) => {
            if is_root {
                object.remove("$recursiveAnchor");
                object.insert(
                    "$dynamicAnchor".to_string(),
                    Value::String(anchor.to_string()),
                );
            }
            if object.remove("$recursiveRef").is_some() {
                object.insert(
                    "$dynamicRef".to_string(),
                    Value::String(format!("#{anchor}")),
                );
            }
            for value in object.values_mut() {
                rewrite_recursive_scope(value, anchor, false);
            }
        }
        _ => {}
    }
}

fn component_dynamic_anchor(component_name: &str) -> String {
    let mut anchor = "roundtrip_".to_string();
    for byte in component_name.as_bytes() {
        anchor.push_str(&format!("{byte:02x}"));
    }
    anchor
}

fn legacy_recursive_scope_error(
    component_name: &str,
    schema: &Value,
    dialect: Dialect,
) -> Option<String> {
    if dialect != Dialect::Draft202012 {
        return None;
    }
    let audit = audit_legacy_recursive_scope(schema);
    if !audit.has_legacy_keyword {
        return None;
    }
    let issue = audit.issue.unwrap_or_else(|| RecursiveScopeIssue {
        location: String::new(),
        message: "legacy recursive keywords were not migrated".to_string(),
    });
    Some(format!(
        "source schema invalid at #/components/schemas/{}{}: unsafe legacy recursive scope: {}",
        escape_pointer_segment(component_name),
        issue.location,
        issue.message
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
    if let Some(Value::String(format)) = schema.get_mut("format") {
        *format = normalize_builtin_format(format).to_string();
    }
    // OpenAPI tooling commonly omits `type: object` when `properties` is
    // present, and the analyzer has always emitted a Rust struct for that
    // shape. Pure JSON Schema would still admit every non-object value because
    // `properties` is conditionally applied. Make the OpenAPI object-inference
    // normalization explicit in the schema oracle so generated typing and
    // validation enforce the same domain.
    if !schema.contains_key("type") && schema.contains_key("properties") {
        schema.insert("type".to_string(), Value::String("object".to_string()));
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

fn contains_required_binary_format(schema: &Value, root: &Value) -> bool {
    contains_required_binary_format_inner(schema, root, 0, &mut BTreeSet::new())
}

fn contains_required_binary_format_inner(
    schema: &Value,
    root: &Value,
    depth: usize,
    visited_refs: &mut BTreeSet<String>,
) -> bool {
    if depth > MAX_SYNTHESIS_DEPTH {
        return false;
    }
    let Value::Object(object) = schema else {
        return false;
    };
    if object.get("format").and_then(Value::as_str) == Some("binary") {
        return true;
    }
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        if !visited_refs.insert(reference.to_string()) {
            return false;
        }
        let result = resolve_local_ref(root, reference).is_some_and(|target| {
            contains_required_binary_format_inner(target, root, depth + 1, visited_refs)
        });
        visited_refs.remove(reference);
        if result {
            return true;
        }
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        if object
            .get(keyword)
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches.iter().any(|branch| {
                    contains_required_binary_format_inner(branch, root, depth + 1, visited_refs)
                })
            })
        {
            return true;
        }
    }

    // Every produced array member is part of the model's wire value. A raw
    // binary item therefore makes the array itself a non-JSON contract even
    // when `minItems` permits an empty sample.
    for keyword in ["items", "contains"] {
        if object.get(keyword).is_some_and(|child| {
            schema_contains_binary_format_inner(child, root, depth + 1, visited_refs)
        }) {
            return true;
        }
    }
    if object
        .get("prefixItems")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|child| {
                schema_contains_binary_format_inner(child, root, depth + 1, visited_refs)
            })
        })
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
                    && contains_required_binary_format_inner(child, root, depth + 1, visited_refs)
            })
        })
}

fn schema_contains_binary_format(schema: &Value, root: &Value) -> bool {
    schema_contains_binary_format_inner(schema, root, 0, &mut BTreeSet::new())
}

fn schema_contains_binary_format_inner(
    schema: &Value,
    root: &Value,
    depth: usize,
    visited_refs: &mut BTreeSet<String>,
) -> bool {
    if depth > MAX_SYNTHESIS_DEPTH {
        return false;
    }
    let Value::Object(object) = schema else {
        return false;
    };
    if object.get("format").and_then(Value::as_str) == Some("binary") {
        return true;
    }
    if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
        if !visited_refs.insert(reference.to_string()) {
            return false;
        }
        let result = resolve_local_ref(root, reference).is_some_and(|target| {
            schema_contains_binary_format_inner(target, root, depth + 1, visited_refs)
        });
        visited_refs.remove(reference);
        if result {
            return true;
        }
    }

    for keyword in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if object
            .get(keyword)
            .and_then(Value::as_array)
            .is_some_and(|schemas| {
                schemas.iter().any(|child| {
                    schema_contains_binary_format_inner(child, root, depth + 1, visited_refs)
                })
            })
        {
            return true;
        }
    }
    for keyword in [
        "items",
        "contains",
        "additionalProperties",
        "unevaluatedProperties",
        "not",
        "if",
        "then",
        "else",
        "propertyNames",
    ] {
        if object.get(keyword).is_some_and(|child| {
            schema_contains_binary_format_inner(child, root, depth + 1, visited_refs)
        }) {
            return true;
        }
    }
    for keyword in [
        "properties",
        "patternProperties",
        "dependentSchemas",
        "$defs",
        "definitions",
    ] {
        if object
            .get(keyword)
            .and_then(Value::as_object)
            .is_some_and(|schemas| {
                schemas.values().any(|child| {
                    schema_contains_binary_format_inner(child, root, depth + 1, visited_refs)
                })
            })
        {
            return true;
        }
    }
    false
}

struct SyntheticGenerator<'a> {
    root: &'a Value,
    dynamic_anchors: BTreeMap<String, &'a Value>,
    validators: Option<&'a jsonschema::ValidatorMap>,
}

impl<'a> SyntheticGenerator<'a> {
    #[cfg(test)]
    fn new(root: &'a Value) -> Self {
        Self::new_inner(root, None)
    }

    fn with_validators(root: &'a Value, validators: &'a jsonschema::ValidatorMap) -> Self {
        Self::new_inner(root, Some(validators))
    }

    fn new_inner(root: &'a Value, validators: Option<&'a jsonschema::ValidatorMap>) -> Self {
        let mut dynamic_anchors = BTreeMap::new();
        collect_dynamic_anchors(root, &mut dynamic_anchors);
        Self {
            root,
            dynamic_anchors,
            validators,
        }
    }

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
        let contains_raw_binary = schema_contains_binary_format(schema, self.root);

        if !contains_raw_binary
            && seed == 0
            && let Some(default) = object.get("default")
        {
            return Some(default.clone());
        }
        if !contains_raw_binary
            && seed == 1
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
        if let Some(reference) = object.get("$dynamicRef").and_then(Value::as_str) {
            let anchor = reference
                .strip_prefix('#')
                .filter(|anchor| !anchor.is_empty() && !anchor.contains('/'))?;
            let target = *self.dynamic_anchors.get(anchor)?;
            let recursion_key = format!("$dynamicRef:{reference}");
            if refs.iter().filter(|seen| *seen == &recursion_key).count()
                >= MAX_DYNAMIC_REF_OCCURRENCES
            {
                return None;
            }
            refs.push(recursion_key);
            let generated = self.generate(target, seed + 2, depth + 1, refs);
            refs.pop();
            return generated;
        }
        if let Some(branches) = object.get("oneOf").and_then(Value::as_array) {
            if let Some(discriminator) = object.get("discriminator").and_then(Value::as_object) {
                return self.generate_discriminated_branch(
                    branches,
                    discriminator,
                    seed,
                    depth,
                    refs,
                );
            }
            return self.generate_branch(branches, seed, depth, refs);
        }
        if let Some(branches) = object.get("anyOf").and_then(Value::as_array) {
            if let Some(discriminator) = object.get("discriminator").and_then(Value::as_object) {
                return self.generate_discriminated_branch(
                    branches,
                    discriminator,
                    seed,
                    depth,
                    refs,
                );
            }
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
            if schema_contains_binary_format(&branches[index], self.root) {
                continue;
            }
            if let Some(value) = self.generate(&branches[index], seed + offset + 2, depth + 1, refs)
            {
                return Some(value);
            }
        }
        None
    }

    fn generate_discriminated_branch(
        &self,
        branches: &[Value],
        discriminator: &Map<String, Value>,
        seed: usize,
        depth: usize,
        refs: &mut Vec<String>,
    ) -> Option<Value> {
        if branches.is_empty() {
            return None;
        }
        let field = discriminator.get("propertyName").and_then(Value::as_str)?;
        let mappings = discriminator.get("mapping").and_then(Value::as_object);

        for offset in 0..branches.len() {
            let index = (seed + offset) % branches.len();
            let branch = &branches[index];
            if schema_contains_binary_format(branch, self.root) {
                continue;
            }
            let Some(candidate) = self.generate(branch, seed + offset + 2, depth + 1, refs) else {
                continue;
            };
            let branch_tags = self.discriminator_tags_for_branch(branch, field);
            let Some(mappings) = mappings else {
                if let Some(tagged) =
                    self.tagged_branch_candidate(branch, &candidate, field, &branch_tags, seed)
                {
                    return Some(tagged);
                }
                continue;
            };
            let matching_keys = mappings
                .iter()
                .filter_map(|(key, target)| {
                    target
                        .as_str()
                        .filter(|target| self.mapping_target_matches_branch(target, branch))
                        .map(|_| key.as_str())
                })
                .collect::<Vec<_>>();
            if matching_keys.is_empty() {
                if self.branch_accepts_candidate(branch, &candidate)
                    && !candidate
                        .get(field)
                        .and_then(Value::as_str)
                        .and_then(|tag| mappings.get(tag))
                        .and_then(Value::as_str)
                        .is_some_and(|target| !self.mapping_target_matches_branch(target, branch))
                {
                    return Some(candidate);
                }
                continue;
            }
            let Value::Object(candidate_object) = &candidate else {
                return Some(candidate);
            };

            for key_offset in 0..matching_keys.len() {
                let key = matching_keys[(seed + key_offset) % matching_keys.len()];
                let mut tagged = candidate_object.clone();
                tagged.insert(field.to_string(), Value::String(key.to_string()));
                let tagged = Value::Object(tagged);
                if self.branch_accepts_candidate(branch, &tagged) {
                    return Some(tagged);
                }
            }

            if let Some(tagged) =
                self.tagged_branch_candidate(branch, &candidate, field, &branch_tags, seed)
            {
                let redirects_elsewhere = tagged
                    .get(field)
                    .and_then(Value::as_str)
                    .and_then(|tag| mappings.get(tag))
                    .and_then(Value::as_str)
                    .is_some_and(|target| !self.mapping_target_matches_branch(target, branch));
                if !redirects_elsewhere {
                    return Some(tagged);
                }
            }

            // A contradictory mapping is only a hint and must not fabricate
            // validity. If the branch generated a value that satisfies its
            // own constraints, keep that schema-faithful value and let the
            // independent union validator decide whether it is usable.
            if self.branch_accepts_candidate(branch, &candidate) {
                return Some(candidate);
            }
        }
        None
    }

    fn tagged_branch_candidate(
        &self,
        branch: &Value,
        candidate: &Value,
        field: &str,
        tags: &[String],
        seed: usize,
    ) -> Option<Value> {
        let Value::Object(candidate_object) = candidate else {
            return self
                .branch_accepts_candidate(branch, candidate)
                .then(|| candidate.clone());
        };
        if tags.is_empty() {
            return self
                .branch_accepts_candidate(branch, candidate)
                .then(|| candidate.clone());
        }
        for offset in 0..tags.len() {
            let tag = &tags[(seed + offset) % tags.len()];
            let mut tagged = candidate_object.clone();
            tagged.insert(field.to_string(), Value::String(tag.clone()));
            let tagged = Value::Object(tagged);
            if self.branch_accepts_candidate(branch, &tagged) {
                return Some(tagged);
            }
        }
        None
    }

    fn discriminator_tags_for_branch(&self, branch: &Value, field: &str) -> Vec<String> {
        if let Some(domain) = self.discriminator_field_domain(branch, field, &mut BTreeSet::new())
            && !domain.is_empty()
        {
            return domain;
        }
        branch
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|reference| reference.rsplit('/').next())
            .map(Self::implicit_discriminator_tag)
            .into_iter()
            .collect()
    }

    fn discriminator_field_domain(
        &self,
        schema: &Value,
        field: &str,
        visited_refs: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        let object = schema.as_object()?;
        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            if !visited_refs.insert(reference.to_string()) {
                return None;
            }
            let result = resolve_local_ref(self.root, reference)
                .and_then(|target| self.discriminator_field_domain(target, field, visited_refs));
            visited_refs.remove(reference);
            return result;
        }

        let own_domain = object
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|properties| properties.get(field))
            .and_then(|property| self.string_constraint_domain(property, visited_refs));
        let composition_domain =
            if let Some(branches) = object.get("allOf").and_then(Value::as_array) {
                branches.iter().fold(None, |domain, branch| {
                    Self::intersect_string_domains(
                        domain,
                        self.discriminator_field_domain(branch, field, visited_refs),
                    )
                })
            } else if let Some(branches) = object
                .get("anyOf")
                .or_else(|| object.get("oneOf"))
                .and_then(Value::as_array)
            {
                self.union_string_domains(branches, field, visited_refs)
            } else {
                None
            };
        Self::intersect_string_domains(own_domain, composition_domain)
    }

    fn string_constraint_domain(
        &self,
        schema: &Value,
        visited_refs: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        let object = schema.as_object()?;
        if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
            if !visited_refs.insert(reference.to_string()) {
                return None;
            }
            let result = resolve_local_ref(self.root, reference)
                .and_then(|target| self.string_constraint_domain(target, visited_refs));
            visited_refs.remove(reference);
            return result;
        }
        let own_domain = object
            .get("const")
            .and_then(Value::as_str)
            .map(|value| vec![value.to_string()])
            .or_else(|| {
                object.get("enum").and_then(Value::as_array).map(|values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
            });
        let composition_domain =
            if let Some(branches) = object.get("allOf").and_then(Value::as_array) {
                branches.iter().fold(None, |domain, branch| {
                    Self::intersect_string_domains(
                        domain,
                        self.string_constraint_domain(branch, visited_refs),
                    )
                })
            } else if let Some(branches) = object
                .get("anyOf")
                .or_else(|| object.get("oneOf"))
                .and_then(Value::as_array)
            {
                let mut values = Vec::new();
                for branch in branches {
                    for value in self.string_constraint_domain(branch, visited_refs)? {
                        if !values.contains(&value) {
                            values.push(value);
                        }
                    }
                }
                Some(values)
            } else {
                None
            };
        Self::intersect_string_domains(own_domain, composition_domain)
    }

    fn union_string_domains(
        &self,
        branches: &[Value],
        field: &str,
        visited_refs: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        let mut values = Vec::new();
        for branch in branches {
            for value in self.discriminator_field_domain(branch, field, visited_refs)? {
                if !values.contains(&value) {
                    values.push(value);
                }
            }
        }
        Some(values)
    }

    fn intersect_string_domains(
        left: Option<Vec<String>>,
        right: Option<Vec<String>>,
    ) -> Option<Vec<String>> {
        match (left, right) {
            (None, other) | (other, None) => other,
            (Some(left), Some(right)) => Some(
                left.into_iter()
                    .filter(|value| right.contains(value))
                    .collect(),
            ),
        }
    }

    fn implicit_discriminator_tag(schema_name: &str) -> String {
        let mut result = String::new();
        let mut chars = schema_name.chars().peekable();
        let mut first = true;
        while let Some(character) = chars.next() {
            if character.is_uppercase()
                && !first
                && chars.peek().is_some_and(|next| next.is_lowercase())
            {
                result.push('.');
            }
            result.push(character.to_ascii_lowercase());
            first = false;
        }
        if result.ends_with("event") {
            result.truncate(result.len() - "event".len());
        }
        if schema_name.starts_with("Response") && !result.starts_with("response.") {
            result = format!("response.{}", result.trim_start_matches("response"));
        }
        result
    }

    fn mapping_target_matches_branch(&self, target: &str, branch: &Value) -> bool {
        let Some(branch_reference) = branch.get("$ref").and_then(Value::as_str) else {
            return false;
        };
        self.normalized_component_reference(target) == branch_reference
            || self.normalized_component_reference(branch_reference) == target
    }

    fn normalized_component_reference(&self, reference: &str) -> String {
        let Some(name) = reference.strip_prefix("#/components/schemas/") else {
            return reference.to_string();
        };
        let definitions_key = if self.root.get("$defs").is_some() {
            "$defs"
        } else {
            "definitions"
        };
        format!("#/{definitions_key}/{name}")
    }

    fn branch_accepts_candidate(&self, branch: &Value, candidate: &Value) -> bool {
        let Some(reference) = branch.get("$ref").and_then(Value::as_str) else {
            // Inline branches still receive independent validation against the
            // containing component before becoming round-trip samples.
            return true;
        };
        self.validators
            .and_then(|validators| validators.get(reference))
            .is_none_or(|validator| validator.is_valid(candidate))
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
            .filter(|(name, child)| {
                !required.contains(*name) && !schema_contains_binary_format(child, self.root)
            })
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
            && !schema_contains_binary_format(additional, self.root)
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

fn collect_dynamic_anchors<'a>(value: &'a Value, anchors: &mut BTreeMap<String, &'a Value>) {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_dynamic_anchors(value, anchors);
            }
        }
        Value::Object(object) => {
            if let Some(anchor) = object.get("$dynamicAnchor").and_then(Value::as_str) {
                anchors.entry(anchor.to_string()).or_insert(value);
            }
            for value in object.values() {
                collect_dynamic_anchors(value, anchors);
            }
        }
        _ => {}
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
            if schema_contains_binary_format(child, generator.root) {
                return None;
            }
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
        Some("uuid") => "123e4567-e89b-42d3-a456-426614174000".to_string(),
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

    fn recursive_compound_filter_schema() -> Value {
        json!({
            "$recursiveAnchor": true,
            "type": "object",
            "additionalProperties": false,
            "required": ["type", "filters"],
            "properties": {
                "type": { "type": "string", "enum": ["and", "or"] },
                "filters": {
                    "type": "array",
                    "items": {
                        "anyOf": [
                            { "$ref": "#/components/schemas/ComparisonFilter" },
                            { "$recursiveRef": "#" }
                        ]
                    }
                }
            }
        })
    }

    #[test]
    fn selects_schema_dialect_from_canonical_openapi_versions() {
        for version in ["3.0", "3.0.0", "3.0.4"] {
            let spec = json!({ "openapi": version });
            assert_eq!(Dialect::from_spec(&spec).expect("draft 4"), Dialect::Draft4);
        }
        for version in ["3.1", "3.1.2", "3.2", "3.2.0"] {
            let spec = json!({ "openapi": version });
            assert_eq!(
                Dialect::from_spec(&spec).expect("draft 2020-12"),
                Dialect::Draft202012
            );
        }
        for version in ["3", "v3.0", "2.0", "3.3.0", "not-a-version"] {
            let spec = json!({ "openapi": version });
            let error = Dialect::from_spec(&spec).expect_err("unsupported version");
            assert!(error.to_string().contains(version), "{error}");
        }
    }

    #[test]
    fn properties_without_type_are_normalized_to_the_analyzers_object_domain() {
        let raw = json!({
            "properties": {
                "status": {"type": "string"},
                "nested": {"properties": {"id": {"type": "integer"}}}
            },
            "example": [{"status": "legacy-array-example"}]
        });
        let normalized = normalize_schema(&raw, Dialect::Draft202012);
        assert_eq!(normalized["type"], "object");
        assert_eq!(normalized["properties"]["nested"]["type"], "object");

        let validator = validator_options(Dialect::Draft202012)
            .build(&normalized)
            .expect("normalized properties-only schema");
        assert!(validator.is_valid(&json!({"status": "delivered"})));
        assert!(!validator.is_valid(&json!([{"status": "delivered"}])));

        let explicit_non_object = normalize_schema(
            &json!({"type": "string", "properties": {"ignored": {"type": "string"}}}),
            Dialect::Draft202012,
        );
        assert_eq!(explicit_non_object["type"], "string");
        let unconstrained = normalize_schema(&json!({"description": "any"}), Dialect::Draft202012);
        assert!(unconstrained.get("type").is_none());
    }

    #[test]
    fn raw_binary_detection_follows_refs_compositions_and_avoids_cycles() {
        let document = json!({
            "$defs": {
                "Binary": { "type": "string", "format": "binary" },
                "BinaryAlias": {
                    "allOf": [{ "$ref": "#/$defs/Binary" }]
                },
                "RequiredUpload": {
                    "type": "object",
                    "required": ["file", "name"],
                    "properties": {
                        "file": {
                            "oneOf": [
                                { "$ref": "#/$defs/BinaryAlias" },
                                { "type": "string", "format": "uri" }
                            ]
                        },
                        "name": { "type": "string" }
                    }
                },
                "OptionalUpload": {
                    "type": "object",
                    "required": ["name"],
                    "default": { "name": "default", "file": "raw-default" },
                    "examples": [{ "name": "example", "file": "raw-example" }],
                    "properties": {
                        "file": { "$ref": "#/$defs/BinaryAlias" },
                        "name": { "type": "string" }
                    }
                },
                "CycleA": { "$ref": "#/$defs/CycleB" },
                "CycleB": {
                    "anyOf": [
                        { "$ref": "#/$defs/CycleA" },
                        { "type": "string" }
                    ]
                }
            }
        });

        assert!(schema_contains_binary_format(
            &document["$defs"]["BinaryAlias"],
            &document
        ));
        assert!(contains_required_binary_format(
            &document["$defs"]["RequiredUpload"],
            &document
        ));
        assert!(!contains_required_binary_format(
            &document["$defs"]["OptionalUpload"],
            &document
        ));
        assert!(!schema_contains_binary_format(
            &document["$defs"]["CycleA"],
            &document
        ));

        let generator = SyntheticGenerator::new(&document);
        for seed in 0..4 {
            let generated = generator
                .generate(
                    &document["$defs"]["OptionalUpload"],
                    seed,
                    0,
                    &mut Vec::new(),
                )
                .expect("optional binary fields must not block safe synthesis");
            assert!(generated.get("name").is_some(), "{generated}");
            assert!(generated.get("file").is_none(), "{generated}");
        }
    }

    #[test]
    fn round_trip_plan_skips_required_binary_but_keeps_optional_siblings() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "raw binary planning", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "Binary": { "type": "string", "format": "binary" },
                "RequiredUpload": {
                    "type": "object",
                    "required": ["file", "name"],
                    "properties": {
                        "file": {
                            "oneOf": [
                                { "$ref": "#/components/schemas/Binary" },
                                { "type": "string", "format": "uri" }
                            ]
                        },
                        "name": { "type": "string" }
                    }
                },
                "OptionalUpload": {
                    "type": "object",
                    "required": ["name"],
                    "examples": [{ "name": "example", "file": "raw-example" }],
                    "properties": {
                        "file": { "$ref": "#/components/schemas/Binary" },
                        "name": { "type": "string" }
                    }
                }
            } }
        });

        let plan = build_round_trip_plan(&spec, 4).expect("binary-aware plan");
        let skipped = plan
            .skipped
            .iter()
            .map(|entry| entry.schema.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(skipped, BTreeSet::from(["Binary", "RequiredUpload"]));
        assert_eq!(plan.stats.tested_schemas, 1);
        assert_eq!(plan.stats.synthesis_skipped_schemas, 2);
        assert!(
            plan.source
                .contains("crate::generated::types::OptionalUpload")
        );
    }

    #[test]
    fn normalizes_uuid_aliases_for_validation_and_v4_synthesis() {
        for format in ["uuid", "uuid4", "uuid_v4", "UUID"] {
            let normalized = normalize_schema(
                &json!({ "type": "string", "format": format }),
                Dialect::Draft202012,
            );
            assert_eq!(normalized.get("format"), Some(&json!("uuid")));

            let validator = validator_options(Dialect::Draft202012)
                .build(&normalized)
                .expect("UUID schema");
            let candidate = SyntheticGenerator::new(&normalized)
                .generate(&normalized, 2, 0, &mut Vec::new())
                .expect("UUID candidate");
            assert_eq!(candidate, json!("123e4567-e89b-42d3-a456-426614174000"));
            assert!(
                validator.is_valid(&candidate),
                "format={format}: {candidate}"
            );
        }
    }

    #[test]
    fn rejects_malformed_uuid_alias_examples_and_preserves_unknown_formats() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "round trip", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "AliasedUuid": {
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "string", "format": "uuid4" }
                    },
                    "examples": [{
                        "id": "e3c6ee77-48cb-416b-b204-11b492cc776e3"
                    }]
                }
            }}
        });
        let plan = build_round_trip_plan(&spec, 2).expect("plan");
        assert_eq!(plan.stats.tested_schemas, 1, "skipped: {:?}", plan.skipped);
        assert_eq!(plan.stats.samples, 1, "malformed example must be rejected");

        let unknown = normalize_schema(
            &json!({ "type": "string", "format": "vendor-id" }),
            Dialect::Draft202012,
        );
        assert_eq!(unknown.get("format"), Some(&json!("vendor-id")));
        let candidate = SyntheticGenerator::new(&unknown)
            .generate(&unknown, 7, 0, &mut Vec::new())
            .expect("unknown-format candidate");
        assert_eq!(candidate, json!("synthetic7"));
    }

    #[test]
    fn atomically_migrates_safe_component_recursive_scope_to_dynamic_keywords() {
        let normalized = normalize_component_schema(
            "CompoundFilter",
            &recursive_compound_filter_schema(),
            Dialect::Draft202012,
        );
        let anchor = "roundtrip_436f6d706f756e6446696c746572";

        assert_eq!(normalized.get("$dynamicAnchor"), Some(&json!(anchor)));
        assert!(normalized.get("$recursiveAnchor").is_none());
        assert_eq!(
            normalized.pointer("/properties/filters/items/anyOf/1/$dynamicRef"),
            Some(&json!(format!("#{anchor}")))
        );
        assert!(
            normalized
                .pointer("/properties/filters/items/anyOf/1/$recursiveRef")
                .is_none()
        );
        jsonschema::draft202012::meta::validate(&normalized)
            .expect("migrated component must satisfy the Draft 2020-12 meta-schema");
    }

    #[test]
    fn leaves_unsafe_recursive_scopes_unchanged_and_quarantines_them() {
        let cases = [
            (
                "NestedId",
                json!({
                    "$recursiveAnchor": true,
                    "properties": {
                        "child": { "$id": "nested", "$recursiveRef": "#" }
                    }
                }),
                "/properties/child/$id",
            ),
            (
                "AnchorConflict",
                json!({
                    "$recursiveAnchor": true,
                    "$anchor": "existing",
                    "properties": { "child": { "$recursiveRef": "#" } }
                }),
                "/$anchor",
            ),
            (
                "NonLocalRef",
                json!({
                    "$recursiveAnchor": true,
                    "properties": {
                        "child": { "$recursiveRef": "#/other" }
                    }
                }),
                "/properties/child/$recursiveRef",
            ),
        ];

        for (name, source, expected_pointer) in cases {
            let normalized = normalize_component_schema(name, &source, Dialect::Draft202012);
            assert_eq!(normalized.get("$recursiveAnchor"), Some(&json!(true)));
            assert!(normalized.get("$dynamicAnchor").is_none());

            let components = [(name.to_string(), normalized)]
                .into_iter()
                .collect::<Map<_, _>>();
            let quarantine = quarantine_invalid_components(&components, Dialect::Draft202012);
            let reason = &quarantine.source_invalid[name];
            assert!(reason.contains("unsafe legacy recursive scope"), "{reason}");
            assert!(reason.contains(expected_pointer), "{reason}");
        }
    }

    #[test]
    fn synthesizes_and_validates_a_bounded_nested_dynamic_filter() {
        let comparison = normalize_component_schema(
            "ComparisonFilter",
            &json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["type", "value"],
                "properties": {
                    "type": { "type": "string", "enum": ["eq"] },
                    "value": { "type": "string" }
                }
            }),
            Dialect::Draft202012,
        );
        let compound = normalize_component_schema(
            "CompoundFilter",
            &recursive_compound_filter_schema(),
            Dialect::Draft202012,
        );
        let document = json!({
            "$schema": Dialect::Draft202012.schema_uri(),
            "$defs": {
                "ComparisonFilter": comparison,
                "CompoundFilter": compound
            }
        });
        let validators = validator_options(Dialect::Draft202012)
            .build_map(&document)
            .expect("recursive validator bundle");
        let validator = validators
            .get("#/$defs/CompoundFilter")
            .expect("CompoundFilter validator");
        let target = document
            .pointer("/$defs/CompoundFilter")
            .expect("CompoundFilter schema");
        let generator = SyntheticGenerator::new(&document);
        let nested = (0..MAX_ATTEMPTS_PER_SCHEMA)
            .filter_map(|seed| generator.generate(target, seed, 0, &mut Vec::new()))
            .find(|candidate| {
                candidate
                    .get("filters")
                    .and_then(Value::as_array)
                    .is_some_and(|filters| {
                        filters.iter().any(|filter| filter.get("filters").is_some())
                    })
            })
            .expect("a bounded nested CompoundFilter sample");
        assert!(
            validator.is_valid(&nested),
            "nested sample {nested}; errors: {:?}",
            validator
                .iter_errors(&nested)
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
        );

        let source = render_test_source(
            &document,
            Dialect::Draft202012,
            &[ModelCase {
                schema_name: "CompoundFilter".to_string(),
                rust_type: "CompoundFilter".to_string(),
                pointer: "#/$defs/CompoundFilter".to_string(),
                samples: vec![nested],
            }],
        )
        .expect("rendered recursive test");
        assert!(source.contains("$dynamicAnchor"));
        assert!(source.contains("$dynamicRef"));
        assert!(source.contains("crate::generated::types::CompoundFilter"));
    }

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
        let candidate = SyntheticGenerator::new(&document)
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
    fn quarantines_invalid_type_and_still_plans_independent_components() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "round trip", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "Bad": { "type": "any" },
                "Independent": { "type": "string", "enum": ["ready"] }
            }}
        });

        let plan = build_round_trip_plan(&spec, 2).expect("plan independent component");
        assert_eq!(plan.stats.component_schemas, 2);
        assert_eq!(plan.stats.tested_schemas, 1, "skipped: {:?}", plan.skipped);
        assert_eq!(plan.stats.source_invalid_schemas, 1);
        assert_eq!(plan.stats.dependent_schemas, 0);
        assert_eq!(plan.stats.synthesis_skipped_schemas, 0);
        assert_eq!(plan.stats.skipped_schemas, 1);
        assert!(plan.source.contains("crate::generated::types::Independent"));
        assert!(!plan.source.contains("crate::generated::types::Bad"));
        assert_eq!(plan.skipped[0].schema, "Bad");
        assert!(
            plan.skipped[0]
                .reason
                .contains("#/components/schemas/Bad/type"),
            "{}",
            plan.skipped[0].reason
        );
        assert!(plan.skipped[0].reason.contains("meta-schema path"));
    }

    #[test]
    fn quarantines_oas30_empty_required_and_one_of_independently() {
        let spec = json!({
            "openapi": "3.0.3",
            "info": { "title": "round trip", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "EmptyOneOf": { "oneOf": [] },
                "EmptyRequired": { "type": "object", "required": [] },
                "Independent": { "type": "integer", "enum": [7] }
            }}
        });

        let plan = build_round_trip_plan(&spec, 2).expect("plan independent component");
        assert_eq!(plan.stats.component_schemas, 3);
        assert_eq!(plan.stats.tested_schemas, 1, "skipped: {:?}", plan.skipped);
        assert_eq!(plan.stats.source_invalid_schemas, 2);
        assert_eq!(plan.stats.dependent_schemas, 0);
        let reasons = plan
            .skipped
            .iter()
            .map(|skipped| skipped.reason.as_str())
            .collect::<Vec<_>>();
        assert!(reasons.iter().any(|reason| reason.contains("/oneOf")));
        assert!(reasons.iter().any(|reason| reason.contains("/required")));
        assert!(plan.source.contains("crate::generated::types::Independent"));
    }

    #[test]
    fn transitively_quarantines_multi_hop_component_dependents() {
        let spec = json!({
            "openapi": "3.1.0",
            "info": { "title": "round trip", "version": "1" },
            "paths": {},
            "components": { "schemas": {
                "Bad": { "type": "any" },
                "Middle": { "$ref": "#/components/schemas/Bad" },
                "Outer": {
                    "type": "object",
                    "required": ["middle"],
                    "properties": {
                        "middle": { "$ref": "#/components/schemas/Middle" }
                    }
                },
                "Independent": { "type": "string", "enum": ["valid"] }
            }}
        });

        let plan = build_round_trip_plan(&spec, 2).expect("plan independent component");
        assert_eq!(plan.stats.component_schemas, 4);
        assert_eq!(plan.stats.tested_schemas, 1, "skipped: {:?}", plan.skipped);
        assert_eq!(plan.stats.source_invalid_schemas, 1);
        assert_eq!(plan.stats.dependent_schemas, 2);
        assert_eq!(plan.stats.synthesis_skipped_schemas, 0);
        assert_eq!(plan.stats.skipped_schemas, 3);
        let skipped = plan
            .skipped
            .iter()
            .map(|skipped| (skipped.schema.as_str(), skipped.reason.as_str()))
            .collect::<BTreeMap<_, _>>();
        assert!(skipped["Middle"].contains("#/components/schemas/Bad"));
        assert!(skipped["Outer"].contains("#/components/schemas/Middle"));
        assert!(plan.source.contains("crate::generated::types::Independent"));
        assert!(!plan.source.contains("crate::generated::types::Middle"));
        assert!(!plan.source.contains("crate::generated::types::Outer"));
    }

    #[test]
    fn serializes_partitioned_round_trip_stats_for_corpus_aggregation() {
        let stats = RoundTripStats {
            component_schemas: 10,
            tested_schemas: 4,
            skipped_schemas: 6,
            source_invalid_schemas: 1,
            dependent_schemas: 2,
            synthesis_skipped_schemas: 3,
            samples: 16,
        };
        assert_eq!(
            stats.to_shell(),
            concat!(
                "component_schemas=10\n",
                "tested_schemas=4\n",
                "skipped_schemas=6\n",
                "source_invalid_schemas=1\n",
                "dependent_schemas=2\n",
                "synthesis_skipped_schemas=3\n",
                "samples=16\n"
            )
        );
    }

    #[test]
    fn discriminated_synthesis_aligns_mapping_tags_with_branch_shapes() {
        let document = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$defs": {
                "BroadKind": { "type": "string", "enum": ["alpha", "beta"] },
                "BroadBase": {
                    "type": "object",
                    "required": ["kind", "id"],
                    "properties": {
                        "kind": { "$ref": "#/$defs/BroadKind" },
                        "id": { "type": "string" }
                    }
                },
                "Alpha": {
                    "allOf": [
                        { "$ref": "#/$defs/BroadBase" },
                        {
                            "type": "object",
                            "required": ["alpha"],
                            "properties": { "alpha": { "type": "string" } }
                        }
                    ]
                },
                "Beta": {
                    "allOf": [
                        { "$ref": "#/$defs/BroadBase" },
                        {
                            "type": "object",
                            "required": ["beta"],
                            "properties": { "beta": { "type": "string" } }
                        }
                    ]
                },
                "Mapped": {
                    "oneOf": [
                        { "$ref": "#/$defs/Alpha" },
                        { "$ref": "#/$defs/Beta" }
                    ],
                    "discriminator": {
                        "propertyName": "kind",
                        "mapping": {
                            "alpha": "#/components/schemas/Alpha",
                            "beta": "#/components/schemas/Beta"
                        }
                    }
                },
                "RoundRobin": {
                    "type": "object",
                    "properties": {
                        "manager_type": { "type": "string", "const": "round_robin" }
                    }
                },
                "Supervisor": {
                    "type": "object",
                    "properties": {
                        "manager_type": { "type": "string", "const": "supervisor" }
                    }
                },
                "OptionalManager": {
                    "oneOf": [
                        { "$ref": "#/$defs/RoundRobin" },
                        { "$ref": "#/$defs/Supervisor" }
                    ],
                    "discriminator": {
                        "propertyName": "manager_type",
                        "mapping": {
                            "round_robin": "#/components/schemas/RoundRobin",
                            "supervisor": "#/components/schemas/Supervisor"
                        }
                    }
                },
                "StrictAlpha": {
                    "type": "object",
                    "required": ["kind", "alpha"],
                    "properties": {
                        "kind": { "type": "string", "const": "alpha" },
                        "alpha": { "type": "string" }
                    }
                },
                "StrictBeta": {
                    "type": "object",
                    "required": ["kind", "beta"],
                    "properties": {
                        "kind": { "type": "string", "const": "beta" },
                        "beta": { "type": "string" }
                    }
                },
                "ContradictoryMapping": {
                    "oneOf": [
                        { "$ref": "#/$defs/StrictAlpha" },
                        { "$ref": "#/$defs/StrictBeta" }
                    ],
                    "discriminator": {
                        "propertyName": "kind",
                        "mapping": {
                            "alpha": "#/components/schemas/StrictBeta",
                            "beta": "#/components/schemas/StrictAlpha"
                        }
                    }
                },
                "ClosedBase": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["kind", "common"],
                    "properties": {
                        "kind": { "type": "string", "enum": ["button", "text", "other"] },
                        "common": { "type": "string" }
                    }
                },
                "ImpossibleButton": {
                    "allOf": [
                        { "$ref": "#/$defs/ClosedBase" },
                        {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": { "label": { "type": "string" } }
                        }
                    ]
                },
                "ClosedMapped": {
                    "oneOf": [
                        { "$ref": "#/$defs/ImpossibleButton" },
                        { "$ref": "#/$defs/ClosedBase" }
                    ],
                    "discriminator": {
                        "propertyName": "kind",
                        "mapping": {
                            "button": "#/components/schemas/ImpossibleButton",
                            "text": "#/components/schemas/ClosedBase",
                            "other": "#/components/schemas/ClosedBase"
                        }
                    }
                },
                "ImplicitAlpha": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "const": "implicit.alpha" },
                        "alpha": { "type": "string" }
                    }
                },
                "ImplicitBeta": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["implicit.beta"] },
                        "beta": { "type": "string" }
                    }
                },
                "ImplicitOptional": {
                    "anyOf": [
                        { "$ref": "#/$defs/ImplicitAlpha" },
                        { "$ref": "#/$defs/ImplicitBeta" }
                    ],
                    "discriminator": { "propertyName": "kind" }
                },
                "mcn_string_item": {
                    "type": "object",
                    "required": ["item_type", "text"],
                    "properties": {
                        "item_type": { "type": "string" },
                        "text": { "type": "string" }
                    }
                },
                "mcn_yaml_item": {
                    "type": "object",
                    "required": ["item_type", "yaml"],
                    "properties": {
                        "item_type": { "type": "string" },
                        "yaml": { "type": "string" }
                    }
                },
                "ImplicitNames": {
                    "anyOf": [
                        { "$ref": "#/$defs/mcn_string_item" },
                        { "$ref": "#/$defs/mcn_yaml_item" }
                    ],
                    "discriminator": { "propertyName": "item_type" }
                }
            }
        });
        let validators = validator_options(Dialect::Draft202012)
            .build_map(&document)
            .expect("validator map");
        let generator = SyntheticGenerator::with_validators(&document, &validators);

        for schema_name in [
            "Mapped",
            "OptionalManager",
            "ContradictoryMapping",
            "ClosedMapped",
            "ImplicitOptional",
            "ImplicitNames",
        ] {
            let schema = &document["$defs"][schema_name];
            let validator = validators
                .get(&format!("#/$defs/{schema_name}"))
                .expect("component validator");
            for seed in 0..8 {
                let candidate = generator
                    .generate(schema, seed, 0, &mut Vec::new())
                    .expect("discriminated candidate");
                assert!(
                    validator.is_valid(&candidate),
                    "{schema_name} seed {seed}: {candidate}"
                );
                let tag = candidate
                    .as_object()
                    .and_then(|object| {
                        object
                            .get("kind")
                            .or_else(|| object.get("manager_type"))
                            .or_else(|| object.get("item_type"))
                    })
                    .and_then(Value::as_str)
                    .expect("mapped candidate tag");
                match tag {
                    "alpha" => assert!(candidate.get("alpha").is_some()),
                    "beta" => assert!(candidate.get("beta").is_some()),
                    "round_robin" | "supervisor" => {}
                    "implicit.alpha" | "implicit.beta" => {}
                    "mcn_string_item" | "mcn_yaml_item" => {}
                    "text" | "other" => {
                        assert!(candidate.get("common").is_some());
                    }
                    other => panic!("unexpected tag {other}"),
                }
            }
        }
    }

    #[test]
    fn coda_column_format_synthesis_avoids_uninhabitable_mapped_branches() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("specs/coda.yaml");
        let body = std::fs::read_to_string(&path).expect("Coda fixture");
        let spec = crate::spec_source::parse_spec(&body, "specs/coda.yaml").expect("Coda spec");
        let dialect = Dialect::from_spec(&spec).expect("Coda dialect");
        let components = spec
            .pointer("/components/schemas")
            .and_then(Value::as_object)
            .expect("Coda components");
        let normalized = components
            .iter()
            .map(|(name, schema)| {
                (
                    name.clone(),
                    normalize_component_schema(name, schema, dialect),
                )
            })
            .collect::<Map<_, _>>();
        let document = json!({
            "$schema": dialect.schema_uri(),
            dialect.definitions_key(): Value::Object(normalized),
        });
        let validators = validator_options(dialect)
            .build_map(&document)
            .expect("Coda validator map");
        let generator = SyntheticGenerator::with_validators(&document, &validators);
        let pointer = format!("#/{}/ColumnFormat", dialect.definitions_key());
        let schema = document
            .pointer(pointer.trim_start_matches('#'))
            .expect("ColumnFormat schema");
        let validator = validators.get(&pointer).expect("ColumnFormat validator");

        for seed in 0..32 {
            let candidate = generator
                .generate(schema, seed, 0, &mut Vec::new())
                .expect("ColumnFormat candidate");
            assert!(validator.is_valid(&candidate), "seed {seed}: {candidate}");
            let tag = candidate["type"].as_str().expect("ColumnFormat tag");
            assert_ne!(tag, "checkbox", "seed {seed}: {candidate}");
            assert_ne!(tag, "button", "seed {seed}: {candidate}");
        }
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
        assert_eq!(plan.stats.synthesis_skipped_schemas, 1);
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
