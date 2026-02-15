use crate::openapi::{Discriminator, OpenApiSpec, Schema, SchemaType as OpenApiSchemaType};
use crate::{GeneratorError, Result};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SchemaAnalysis {
    /// All schemas indexed by name
    pub schemas: BTreeMap<String, AnalyzedSchema>,
    /// Dependency graph for generation ordering
    pub dependencies: DependencyGraph,
    /// Detected patterns and transformations
    pub patterns: DetectedPatterns,
    /// OpenAPI operations and their request/response schemas
    pub operations: BTreeMap<String, OperationInfo>,
}

#[derive(Debug, Clone)]
pub struct AnalyzedSchema {
    pub name: String,
    pub original: Value,
    pub schema_type: SchemaType,
    pub dependencies: HashSet<String>,
    pub nullable: bool,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum SchemaType {
    /// Simple primitive type
    Primitive { rust_type: String },
    /// Object with properties
    Object {
        properties: BTreeMap<String, PropertyInfo>,
        required: HashSet<String>,
        additional_properties: bool,
    },
    /// Discriminated union (oneOf + discriminator)
    DiscriminatedUnion {
        discriminator_field: String,
        variants: Vec<UnionVariant>,
    },
    /// Simple union (anyOf without discriminator)
    Union { variants: Vec<SchemaRef> },
    /// Array type
    Array { item_type: Box<SchemaType> },
    /// String enum
    StringEnum { values: Vec<String> },
    /// Extensible enum with known values and custom variant
    ExtensibleEnum { known_values: Vec<String> },
    /// Schema composition (allOf)
    Composition { schemas: Vec<SchemaRef> },
    /// Reference to another schema
    Reference { target: String },
}

#[derive(Debug, Clone)]
pub struct PropertyInfo {
    pub schema_type: SchemaType,
    pub nullable: bool,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
    pub serde_attrs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UnionVariant {
    pub rust_name: String,
    pub type_name: String,
    pub discriminator_value: String,
    pub schema_ref: String,
}

#[derive(Debug, Clone)]
pub struct SchemaRef {
    pub target: String,
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub struct DependencyGraph {
    pub edges: BTreeMap<String, HashSet<String>>,
    /// Set of schemas that have recursive dependencies
    pub recursive_schemas: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct DetectedPatterns {
    /// Schemas that should use tagged enums (discriminated unions)
    pub tagged_enum_schemas: HashSet<String>,
    /// Schemas that should use untagged enums (simple unions)  
    pub untagged_enum_schemas: HashSet<String>,
    /// Auto-detected type mappings for discriminated unions
    pub type_mappings: BTreeMap<String, BTreeMap<String, String>>,
}

/// Information about an OpenAPI operation
#[derive(Debug, Clone, serde::Serialize)]
pub struct OperationInfo {
    /// Operation ID
    pub operation_id: String,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Path template
    pub path: String,
    /// Request body content type and schema (if any)
    pub request_body: Option<RequestBodyContent>,
    /// Response schemas by status code
    pub response_schemas: BTreeMap<String, String>,
    /// Parameters (path, query, header)
    pub parameters: Vec<ParameterInfo>,
    /// Whether this operation supports streaming
    pub supports_streaming: bool,
    /// Stream parameter name if applicable
    pub stream_parameter: Option<String>,
}

/// Content type and schema for a request body
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum RequestBodyContent {
    Json { schema_name: String },
    FormUrlEncoded { schema_name: String },
    Multipart,
    OctetStream,
    TextPlain,
}

impl RequestBodyContent {
    /// Get the schema name if this content type has one
    pub fn schema_name(&self) -> Option<&str> {
        match self {
            Self::Json { schema_name } | Self::FormUrlEncoded { schema_name } => Some(schema_name),
            _ => None,
        }
    }
}

/// Information about an operation parameter
#[derive(Debug, Clone, serde::Serialize)]
pub struct ParameterInfo {
    /// Parameter name
    pub name: String,
    /// Parameter location (path, query, header, cookie)
    pub location: String,
    /// Whether the parameter is required
    pub required: bool,
    /// Schema reference for the parameter type
    pub schema_ref: Option<String>,
    /// Rust type for this parameter
    pub rust_type: String,
}

impl Default for DependencyGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            edges: BTreeMap::new(),
            recursive_schemas: HashSet::new(),
        }
    }

    pub fn add_dependency(&mut self, from: String, to: String) {
        self.edges.entry(from).or_default().insert(to);
    }

    /// Get topological sort order for generation
    pub fn topological_sort(&mut self) -> Result<Vec<String>> {
        // First, detect and handle recursive dependencies
        self.detect_recursive_schemas();

        // Create a temporary graph without self-referencing edges for sorting
        let mut temp_edges = self.edges.clone();
        for (schema, deps) in &mut temp_edges {
            deps.remove(schema); // Remove self-references
        }

        let mut visited = HashSet::new();
        let mut temp_visited = HashSet::new();
        let mut result = Vec::new();

        // Visit all nodes using the temporary graph in sorted order for deterministic output
        let mut all_nodes: Vec<_> = temp_edges.keys().collect();
        all_nodes.sort();
        for node in all_nodes {
            if !visited.contains(node) {
                self.visit_node_recursive(
                    node,
                    &temp_edges,
                    &mut visited,
                    &mut temp_visited,
                    &mut result,
                )?;
            }
        }

        result.reverse();
        Ok(result)
    }

    fn detect_recursive_schemas(&mut self) {
        for (schema, deps) in &self.edges {
            if deps.contains(schema) {
                // Direct self-reference
                self.recursive_schemas.insert(schema.clone());
            } else {
                // Check for indirect cycles
                if self.has_cycle_from(schema, schema, &mut HashSet::new()) {
                    self.recursive_schemas.insert(schema.clone());
                }
            }
        }

        // Also detect mutual recursion (like GraphNode <-> GraphEdge)
        for (schema, deps) in &self.edges {
            for dep in deps {
                if let Some(dep_deps) = self.edges.get(dep) {
                    if dep_deps.contains(schema) {
                        // Mutual recursion detected
                        self.recursive_schemas.insert(schema.clone());
                        self.recursive_schemas.insert(dep.clone());
                    }
                }
            }
        }
    }

    fn has_cycle_from(&self, start: &str, current: &str, visited: &mut HashSet<String>) -> bool {
        if visited.contains(current) {
            return false; // Already checked this path
        }

        visited.insert(current.to_string());

        if let Some(deps) = self.edges.get(current) {
            for dep in deps {
                if dep == start {
                    return true; // Found cycle back to start
                }
                if self.has_cycle_from(start, dep, visited) {
                    return true;
                }
            }
        }

        false
    }

    #[allow(clippy::only_used_in_recursion)]
    fn visit_node_recursive(
        &self,
        node: &str,
        temp_edges: &BTreeMap<String, HashSet<String>>,
        visited: &mut HashSet<String>,
        temp_visited: &mut HashSet<String>,
        result: &mut Vec<String>,
    ) -> Result<()> {
        if temp_visited.contains(node) {
            // This should not happen with cycle-free temp graph, but just in case
            return Ok(());
        }

        if visited.contains(node) {
            return Ok(());
        }

        temp_visited.insert(node.to_string());

        if let Some(dependencies) = temp_edges.get(node) {
            // Sort dependencies for deterministic topological order
            let mut sorted_deps: Vec<_> = dependencies.iter().collect();
            sorted_deps.sort();
            for dep in sorted_deps {
                self.visit_node_recursive(dep, temp_edges, visited, temp_visited, result)?;
            }
        }

        temp_visited.remove(node);
        visited.insert(node.to_string());
        result.push(node.to_string());

        Ok(())
    }
}

/// Merge schema extension files into the main OpenAPI specification
/// Uses simple recursive JSON object merging
pub fn merge_schema_extensions(
    main_spec: Value,
    extension_paths: &[impl AsRef<Path>],
) -> Result<Value> {
    let mut result = main_spec;

    for path in extension_paths {
        let extension = load_extension_file(path.as_ref())?;
        result = merge_json_objects_with_replacements(result, extension)?;
    }

    Ok(result)
}

/// Load an extension file and parse as JSON
fn load_extension_file(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path).map_err(|e| GeneratorError::FileError {
        message: format!("Failed to read file {}: {}", path.display(), e),
    })?;

    serde_json::from_str(&content).map_err(GeneratorError::ParseError)
}

/// Merge JSON objects with explicit replacement support
fn merge_json_objects_with_replacements(main: Value, extension: Value) -> Result<Value> {
    // Extract replacement rules from the extension
    let replacements = extract_replacement_rules(&extension);

    // Perform the merge with replacement awareness
    Ok(merge_json_objects_with_rules(
        main,
        extension,
        &replacements,
    ))
}

/// Extract x-replacements rules from extension
fn extract_replacement_rules(
    extension: &Value,
) -> std::collections::HashMap<String, (String, String)> {
    let mut rules = std::collections::HashMap::new();

    if let Some(x_replacements) = extension.get("x-replacements") {
        if let Some(x_replacements_obj) = x_replacements.as_object() {
            for (schema_name, replacement_rule) in x_replacements_obj {
                if let Some(rule_obj) = replacement_rule.as_object() {
                    if let (Some(replace), Some(with)) = (
                        rule_obj.get("replace").and_then(|v| v.as_str()),
                        rule_obj.get("with").and_then(|v| v.as_str()),
                    ) {
                        rules.insert(schema_name.clone(), (replace.to_string(), with.to_string()));
                        // println!("📋 Replacement rule: In {}, replace {} with {}", schema_name, replace, with);
                    }
                }
            }
        }
    }

    rules
}

/// Check if a variant should be replaced based on explicit replacement rules
fn should_replace_variant(
    schema_name: &str,
    extension_refs: &[String],
    replacements: &std::collections::HashMap<String, (String, String)>,
) -> bool {
    // Check all replacement rules
    for (replace_schema, with_schema) in replacements.values() {
        if schema_name == replace_schema {
            // This schema should be replaced - check if the replacement schema is in extensions
            let replacement_exists = extension_refs.iter().any(|ext_ref| {
                let ext_schema_name = ext_ref.split('/').next_back().unwrap_or("");
                ext_schema_name == with_schema
            });

            if replacement_exists {
                return true;
            }
        }
    }

    // Fallback to exact name match for complete replacement
    extension_refs.iter().any(|ext_ref| {
        let ext_schema_name = ext_ref.split('/').next_back().unwrap_or("");
        schema_name == ext_schema_name
    })
}

/// Recursively merge two JSON values with replacement rules
/// Objects are merged by combining properties
/// Arrays are merged by concatenating
/// Primitives in the extension override the main value
fn merge_json_objects_with_rules(
    main: Value,
    extension: Value,
    replacements: &std::collections::HashMap<String, (String, String)>,
) -> Value {
    match (main, extension) {
        // Both objects - merge properties
        (Value::Object(mut main_obj), Value::Object(ext_obj)) => {
            // Special handling for schema objects with oneOf/anyOf variants
            if let (Some(main_variants), Some(ext_variants)) = (
                extract_schema_variants(&Value::Object(main_obj.clone())),
                extract_schema_variants(&Value::Object(ext_obj.clone())),
            ) {
                println!(
                    "🔍 Merging union schemas: {} main variants, {} extension variants",
                    main_variants.len(),
                    ext_variants.len()
                );
                // Merge the variant arrays and use oneOf as the canonical key
                // First, collect main variants, but filter out any that will be replaced by extension
                let mut merged_variants = Vec::new();
                let extension_refs: Vec<String> = ext_variants
                    .iter()
                    .filter_map(|v| v.get("$ref").and_then(|r| r.as_str()))
                    .map(|s| s.to_string())
                    .collect();

                // Add main variants that aren't being replaced
                for main_variant in main_variants {
                    if let Some(main_ref) = main_variant.get("$ref").and_then(|r| r.as_str()) {
                        // Check if this main variant should be replaced by an extension variant
                        let schema_name = main_ref.split('/').next_back().unwrap_or("");
                        let should_replace =
                            should_replace_variant(schema_name, &extension_refs, replacements);

                        if should_replace {
                            println!("🔄 REPLACING {} (explicit rule)", schema_name);
                        }

                        if !should_replace {
                            merged_variants.push(main_variant);
                        }
                    } else {
                        // Keep non-ref variants
                        merged_variants.push(main_variant);
                    }
                }

                // Add all extension variants
                for ext_variant in ext_variants {
                    merged_variants.push(ext_variant);
                }

                // Remove old oneOf/anyOf keys and add the merged oneOf
                main_obj.remove("oneOf");
                main_obj.remove("anyOf");
                main_obj.insert("oneOf".to_string(), Value::Array(merged_variants));

                // Merge other properties normally
                for (key, ext_value) in ext_obj {
                    if key != "oneOf" && key != "anyOf" {
                        match main_obj.get(&key) {
                            Some(main_value) => {
                                let merged_value = merge_json_objects_with_rules(
                                    main_value.clone(),
                                    ext_value,
                                    replacements,
                                );
                                main_obj.insert(key, merged_value);
                            }
                            None => {
                                main_obj.insert(key, ext_value);
                            }
                        }
                    }
                }

                return Value::Object(main_obj);
            }

            // Normal object merging
            for (key, ext_value) in ext_obj {
                match main_obj.get(&key) {
                    Some(main_value) => {
                        // Key exists in both - recursively merge
                        let merged_value = merge_json_objects_with_rules(
                            main_value.clone(),
                            ext_value,
                            replacements,
                        );
                        main_obj.insert(key, merged_value);
                    }
                    None => {
                        // Key only in extension - add it
                        main_obj.insert(key, ext_value);
                    }
                }
            }
            Value::Object(main_obj)
        }

        // Both arrays - concatenate
        (Value::Array(mut main_arr), Value::Array(ext_arr)) => {
            main_arr.extend(ext_arr);
            Value::Array(main_arr)
        }

        // Extension overrides main for all other cases
        (_, extension) => extension,
    }
}

/// Extract schema variants from oneOf or anyOf properties
fn extract_schema_variants(obj: &Value) -> Option<Vec<Value>> {
    if let Value::Object(map) = obj {
        if let Some(Value::Array(variants)) = map.get("oneOf") {
            return Some(variants.clone());
        }
        if let Some(Value::Array(variants)) = map.get("anyOf") {
            return Some(variants.clone());
        }
    }
    None
}

pub struct SchemaAnalyzer {
    schemas: BTreeMap<String, Schema>,
    resolved_cache: BTreeMap<String, AnalyzedSchema>,
    openapi_spec: Value,
    current_schema_name: Option<String>,
}

impl SchemaAnalyzer {
    pub fn new(openapi_spec: Value) -> Result<Self> {
        let spec: OpenApiSpec =
            serde_json::from_value(openapi_spec.clone()).map_err(GeneratorError::ParseError)?;
        let schemas = Self::extract_schemas(&spec)?;

        Ok(Self {
            schemas,
            resolved_cache: BTreeMap::new(),
            openapi_spec,
            current_schema_name: None,
        })
    }

    /// Create a new analyzer with schema extensions merged in
    pub fn new_with_extensions(
        openapi_spec: Value,
        extension_paths: &[std::path::PathBuf],
    ) -> Result<Self> {
        let merged_spec = merge_schema_extensions(openapi_spec, extension_paths)?;
        Self::new(merged_spec)
    }

    /// Generate a context-aware name for inline types, arrays, and variants
    /// This provides better naming than generic names like UnionArray1, InlineVariant2, etc.
    fn generate_context_aware_name(
        &self,
        base_context: &str,
        type_hint: &str,
        index: usize,
        schema: Option<&Schema>,
    ) -> String {
        // First, try to infer a better name from the schema structure
        if let Some(schema) = schema {
            // For arrays, check if we can derive name from items
            if type_hint == "Array"
                && matches!(schema.schema_type(), Some(OpenApiSchemaType::Array))
            {
                if let Some(items_schema) = &schema.details().items {
                    // Check for specific item types
                    if let Some(item_type) = items_schema.schema_type() {
                        match item_type {
                            OpenApiSchemaType::Object => {
                                return format!("{base_context}ItemArray");
                            }
                            OpenApiSchemaType::String => {
                                return format!("{base_context}StringArray");
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Generate context-aware name based on type hint
        match type_hint {
            "Array" => {
                // For arrays, always use context name instead of generic numbering
                format!("{base_context}Array")
            }
            "Variant" | "InlineVariant" => {
                // For variants, include index only if > 0 to keep first variant clean
                if index == 0 {
                    format!("{base_context}{type_hint}")
                } else {
                    format!("{}{}{}", base_context, type_hint, index + 1)
                }
            }
            _ => {
                // Default case
                format!("{base_context}{type_hint}{index}")
            }
        }
    }

    /// Convert a string to PascalCase, handling underscores and hyphens
    fn to_pascal_case(&self, s: &str) -> String {
        s.split(['_', '-'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    None => String::new(),
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect()
    }

    fn extract_schemas(spec: &OpenApiSpec) -> Result<BTreeMap<String, Schema>> {
        let schemas = spec
            .components
            .as_ref()
            .and_then(|c| c.schemas.as_ref())
            .ok_or_else(|| {
                GeneratorError::InvalidSchema("No schemas found in OpenAPI spec".to_string())
            })?;

        // Convert BTreeMap to BTreeMap for deterministic iteration order
        Ok(schemas
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    pub fn analyze(&mut self) -> Result<SchemaAnalysis> {
        let mut analysis = SchemaAnalysis {
            schemas: BTreeMap::new(),
            dependencies: DependencyGraph::new(),
            patterns: DetectedPatterns {
                tagged_enum_schemas: HashSet::new(),
                untagged_enum_schemas: HashSet::new(),
                type_mappings: BTreeMap::new(),
            },
            operations: BTreeMap::new(),
        };

        // First pass: detect patterns
        self.detect_patterns(&mut analysis.patterns)?;

        // Second pass: analyze each schema
        let schema_names: Vec<String> = self.schemas.keys().cloned().collect();
        for schema_name in schema_names {
            let analyzed = self.analyze_schema(&schema_name)?;

            // Build dependency graph
            for dep in &analyzed.dependencies {
                analysis
                    .dependencies
                    .add_dependency(schema_name.clone(), dep.clone());
            }

            analysis.schemas.insert(schema_name, analyzed);
        }

        // Third pass: include any inline schemas that were generated during analysis
        // BTreeMap maintains sorted order, so iteration is deterministic
        for (inline_name, inline_schema) in &self.resolved_cache {
            if !analysis.schemas.contains_key(inline_name) {
                // Add the inline schema first
                analysis
                    .schemas
                    .insert(inline_name.clone(), inline_schema.clone());

                // Build dependency graph for inline schema's own dependencies
                for dep in &inline_schema.dependencies {
                    analysis
                        .dependencies
                        .add_dependency(inline_name.clone(), dep.clone());
                }

                // Check if any existing schemas depend on this inline schema
                // We need to check ALL schemas, not just the ones already in analysis.schemas,
                // because parent schemas might have been analyzed but their dependencies
                // on inline schemas might not have been added to the dependency graph yet
                let mut schemas_to_update = Vec::new();
                for (schema_name, schema) in &analysis.schemas {
                    // Skip self-reference
                    if schema_name == inline_name {
                        continue;
                    }

                    if schema.dependencies.contains(inline_name) {
                        // The parent schema depends on this inline schema
                        schemas_to_update.push(schema_name.clone());
                    }
                }

                // Add the dependencies to the graph
                for schema_name in schemas_to_update {
                    analysis
                        .dependencies
                        .add_dependency(schema_name, inline_name.clone());
                }
            }
        }

        // Fourth pass: analyze OpenAPI operations
        self.analyze_operations(&mut analysis)?;

        // Fifth pass: include any inline schemas generated during operation analysis
        // (e.g., inline response types)
        for (inline_name, inline_schema) in &self.resolved_cache {
            if !analysis.schemas.contains_key(inline_name) {
                analysis
                    .schemas
                    .insert(inline_name.clone(), inline_schema.clone());

                // Build dependency graph for inline schema's dependencies
                for dep in &inline_schema.dependencies {
                    analysis
                        .dependencies
                        .add_dependency(inline_name.clone(), dep.clone());
                }
            }
        }

        Ok(analysis)
    }

    fn detect_patterns(&self, patterns: &mut DetectedPatterns) -> Result<()> {
        for (schema_name, schema) in &self.schemas {
            // Detect discriminated unions
            if self.is_discriminated_union(schema) {
                patterns.tagged_enum_schemas.insert(schema_name.clone());

                // Extract type mappings for this union
                if let Some(mappings) = self.extract_type_mappings(schema)? {
                    patterns.type_mappings.insert(schema_name.clone(), mappings);
                }
            }
            // Detect simple unions
            else if self.is_simple_union(schema) {
                patterns.untagged_enum_schemas.insert(schema_name.clone());
            }
        }

        Ok(())
    }

    fn is_discriminated_union(&self, schema: &Schema) -> bool {
        // Check for explicit discriminator
        if schema.is_discriminated_union() {
            return true;
        }

        // Auto-detect from union patterns with any common const field
        if let Some(variants) = schema.union_variants() {
            return variants.len() > 2 && self.detect_discriminator_field(variants).is_some();
        }

        false
    }

    fn all_variants_have_const_field(&self, variants: &[Schema], field_name: &str) -> bool {
        variants.iter().all(|variant| {
            if let Some(ref_str) = variant.reference() {
                // $ref variant: resolve and check the referenced schema
                if let Some(schema_name) = self.extract_schema_name(ref_str) {
                    if let Some(schema) = self.schemas.get(schema_name) {
                        return self.has_const_discriminator_field(schema, field_name);
                    }
                }
            } else {
                // Inline variant: check properties directly
                return self.has_const_discriminator_field(variant, field_name);
            }
            false
        })
    }

    /// Scan all variants to find any common property that has a const/single-enum value
    /// across all variants. Returns the field name if found.
    /// Prioritizes "type" if it matches (most common convention).
    fn detect_discriminator_field(&self, variants: &[Schema]) -> Option<String> {
        if variants.is_empty() {
            return None;
        }

        // Collect candidate field names from the first variant
        let first_variant = &variants[0];
        let first_schema = if let Some(ref_str) = first_variant.reference() {
            let schema_name = self.extract_schema_name(ref_str)?;
            self.schemas.get(schema_name)?
        } else {
            first_variant
        };

        let properties = first_schema.details().properties.as_ref()?;
        let mut candidates: Vec<String> = Vec::new();

        for (field_name, field_schema) in properties {
            let details = field_schema.details();
            let is_const = details.const_value.is_some()
                || details.enum_values.as_ref().is_some_and(|v| v.len() == 1)
                || details.extra.contains_key("const");
            if is_const {
                candidates.push(field_name.clone());
            }
        }

        if candidates.is_empty() {
            return None;
        }

        // Prioritize "type" if it's among candidates
        candidates.sort_by(|a, b| {
            if a == "type" {
                std::cmp::Ordering::Less
            } else if b == "type" {
                std::cmp::Ordering::Greater
            } else {
                a.cmp(b)
            }
        });

        // Check each candidate against all variants
        for candidate in &candidates {
            if self.all_variants_have_const_field(variants, candidate) {
                return Some(candidate.clone());
            }
        }

        None
    }

    fn has_const_discriminator_field(&self, schema: &Schema, field_name: &str) -> bool {
        if let Some(properties) = &schema.details().properties {
            if let Some(field) = properties.get(field_name) {
                // Check for const value (OpenAPI 3.1 style)
                if field.details().const_value.is_some() {
                    return true;
                }
                // Check if it's an enum field with a single value
                if let Some(enum_vals) = &field.details().enum_values {
                    return enum_vals.len() == 1;
                }
                // Fallback: check extra fields for const
                return field.details().extra.contains_key("const");
            }
        }
        false
    }

    fn is_simple_union(&self, schema: &Schema) -> bool {
        if let Some(variants) = schema.union_variants() {
            // Simple union: multiple types but not nullable pattern
            if variants.len() > 1 && !schema.is_nullable_pattern() {
                let has_refs = variants.iter().any(|v| v.is_reference());
                return has_refs;
            }
        }
        false
    }

    fn extract_type_mappings(&self, schema: &Schema) -> Result<Option<BTreeMap<String, String>>> {
        let variants = schema.union_variants().ok_or_else(|| {
            GeneratorError::InvalidSchema("No variants found for discriminated union".to_string())
        })?;

        // Get the discriminator field name from the schema
        let discriminator_field = if let Some(discriminator) = schema.discriminator() {
            discriminator.property_name.clone()
        } else if let Some(detected) = self.detect_discriminator_field(variants) {
            detected
        } else {
            "type".to_string() // fallback to "type" for auto-detected discriminated unions
        };

        let mut mappings = BTreeMap::new();

        for variant in variants {
            if let Some(ref_str) = variant.reference() {
                if let Some(type_name) = self.extract_schema_name(ref_str) {
                    if let Some(variant_schema) = self.schemas.get(type_name) {
                        if let Some(discriminator_value) = self
                            .extract_discriminator_value_for_field(
                                variant_schema,
                                &discriminator_field,
                            )
                        {
                            mappings.insert(type_name.to_string(), discriminator_value);
                        }
                    }
                }
            }
        }

        if mappings.is_empty() {
            Ok(None)
        } else {
            Ok(Some(mappings))
        }
    }

    #[allow(dead_code)]
    fn extract_discriminator_value(&self, schema: &Schema) -> Option<String> {
        self.extract_discriminator_value_for_field(schema, "type")
    }

    fn extract_discriminator_value_for_field(
        &self,
        schema: &Schema,
        field_name: &str,
    ) -> Option<String> {
        if let Some(properties) = &schema.details().properties {
            if let Some(type_field) = properties.get(field_name) {
                // Check for const value first (highest priority)
                if let Some(const_value) = &type_field.details().const_value {
                    if let Some(value) = const_value.as_str() {
                        return Some(value.to_string());
                    }
                }
                // Check for enum with single value
                if let Some(enum_values) = &type_field.details().enum_values {
                    if enum_values.len() == 1 {
                        return enum_values[0].as_str().map(|s| s.to_string());
                    }
                }
                // Check for const value in extra fields
                if let Some(const_value) = type_field.details().extra.get("const") {
                    return const_value.as_str().map(|s| s.to_string());
                }
                // Check for x-stainless-const with default value
                if let Some(stainless_const) = type_field.details().extra.get("x-stainless-const") {
                    if stainless_const.as_bool() == Some(true) {
                        if let Some(default_value) = &type_field.details().default {
                            if let Some(value) = default_value.as_str() {
                                return Some(value.to_string());
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn get_any_reference<'a>(&self, schema: &'a Schema) -> Option<&'a str> {
        schema.reference().or_else(|| schema.recursive_reference())
    }

    fn extract_schema_name<'a>(&self, ref_str: &'a str) -> Option<&'a str> {
        if ref_str == "#" {
            None // Special case for self-reference
        } else {
            ref_str.split('/').next_back()
        }
    }

    fn analyze_schema(&mut self, schema_name: &str) -> Result<AnalyzedSchema> {
        // Check cache first
        if let Some(cached) = self.resolved_cache.get(schema_name) {
            return Ok(cached.clone());
        }

        // Set current schema name for context
        self.current_schema_name = Some(schema_name.to_string());

        let schema = self
            .schemas
            .get(schema_name)
            .ok_or_else(|| GeneratorError::UnresolvedReference(schema_name.to_string()))?
            .clone();

        // Prevent infinite recursion with placeholder
        self.resolved_cache.insert(
            schema_name.to_string(),
            AnalyzedSchema {
                name: schema_name.to_string(),
                original: serde_json::to_value(&schema).unwrap_or(Value::Null),
                schema_type: SchemaType::Reference {
                    target: "placeholder".to_string(),
                },
                dependencies: HashSet::new(),
                nullable: false,
                description: None,
                default: None,
            },
        );

        let analyzed = self.analyze_schema_value(&schema, schema_name)?;

        // Update cache with real result
        self.resolved_cache
            .insert(schema_name.to_string(), analyzed.clone());

        Ok(analyzed)
    }

    fn analyze_schema_value(
        &mut self,
        schema: &Schema,
        schema_name: &str,
    ) -> Result<AnalyzedSchema> {
        let details = schema.details();
        let description = details.description.clone();
        let nullable = details.is_nullable();
        let mut dependencies = HashSet::new();

        let schema_type = match schema {
            Schema::Reference { reference, .. } => {
                let target = self
                    .extract_schema_name(reference)
                    .ok_or_else(|| GeneratorError::UnresolvedReference(reference.to_string()))?
                    .to_string();
                dependencies.insert(target.clone());
                SchemaType::Reference { target }
            }
            Schema::RecursiveRef { recursive_ref, .. } => {
                // Handle recursive references
                if recursive_ref == "#" {
                    // Self-reference to the current schema
                    dependencies.insert(schema_name.to_string());
                    SchemaType::Reference {
                        target: schema_name.to_string(),
                    }
                } else {
                    // Handle other recursive reference patterns
                    let target = self
                        .extract_schema_name(recursive_ref)
                        .unwrap_or(schema_name)
                        .to_string();
                    dependencies.insert(target.clone());
                    SchemaType::Reference { target }
                }
            }
            Schema::Typed { schema_type, .. } => {
                match schema_type {
                    OpenApiSchemaType::String => {
                        if let Some(values) = details.string_enum_values() {
                            SchemaType::StringEnum { values }
                        } else {
                            SchemaType::Primitive {
                                rust_type: "String".to_string(),
                            }
                        }
                    }
                    OpenApiSchemaType::Integer => {
                        let rust_type =
                            self.get_number_rust_type(OpenApiSchemaType::Integer, details);
                        SchemaType::Primitive { rust_type }
                    }
                    OpenApiSchemaType::Number => {
                        let rust_type =
                            self.get_number_rust_type(OpenApiSchemaType::Number, details);
                        SchemaType::Primitive { rust_type }
                    }
                    OpenApiSchemaType::Boolean => SchemaType::Primitive {
                        rust_type: "bool".to_string(),
                    },
                    OpenApiSchemaType::Array => {
                        // Analyze array item type
                        self.analyze_array_schema(schema, schema_name, &mut dependencies)?
                    }
                    OpenApiSchemaType::Object => {
                        // Check if this is a dynamic JSON object
                        if self.should_use_dynamic_json(schema) {
                            SchemaType::Primitive {
                                rust_type: "serde_json::Value".to_string(),
                            }
                        } else {
                            // Analyze object properties
                            self.analyze_object_schema(schema, &mut dependencies)?
                        }
                    }
                    _ => SchemaType::Primitive {
                        rust_type: "serde_json::Value".to_string(),
                    },
                }
            }
            Schema::AnyOf {
                any_of,
                discriminator,
                ..
            } => {
                // Handle anyOf patterns (nullable vs flexible union vs discriminated)
                self.analyze_anyof_union(
                    any_of,
                    discriminator.as_ref(),
                    &mut dependencies,
                    schema_name,
                )?
            }
            Schema::OneOf {
                one_of,
                discriminator,
                ..
            } => {
                // Handle oneOf discriminated unions
                self.analyze_oneof_union(one_of, discriminator.as_ref(), None, &mut dependencies)?
            }
            Schema::AllOf { all_of, .. } => {
                // Handle allOf composition (schema inheritance)
                self.analyze_allof_composition(all_of, &mut dependencies)?
            }
            Schema::Untyped { .. } => {
                // Try to infer type from structure
                if let Some(inferred) = schema.inferred_type() {
                    match inferred {
                        OpenApiSchemaType::Object => {
                            if self.should_use_dynamic_json(schema) {
                                SchemaType::Primitive {
                                    rust_type: "serde_json::Value".to_string(),
                                }
                            } else {
                                self.analyze_object_schema(schema, &mut dependencies)?
                            }
                        }
                        OpenApiSchemaType::String if details.is_string_enum() => {
                            SchemaType::StringEnum {
                                values: details.string_enum_values().unwrap_or_default(),
                            }
                        }
                        _ => SchemaType::Primitive {
                            rust_type: "serde_json::Value".to_string(),
                        },
                    }
                } else {
                    SchemaType::Primitive {
                        rust_type: "serde_json::Value".to_string(),
                    }
                }
            }
        };

        Ok(AnalyzedSchema {
            name: schema_name.to_string(),
            original: serde_json::to_value(schema).unwrap_or(Value::Null), // Convert back to Value for now
            schema_type,
            dependencies,
            nullable,
            description,
            default: details.default.clone(),
        })
    }

    fn analyze_object_schema(
        &mut self,
        schema: &Schema,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        let details = schema.details();
        let properties = &details.properties;
        let required = details
            .required
            .as_ref()
            .map(|req| req.iter().cloned().collect::<HashSet<String>>())
            .unwrap_or_default();

        let mut property_info = BTreeMap::new();

        if let Some(props) = properties {
            for (prop_name, prop_schema) in props {
                // Check if this property is a union that needs a named type
                let prop_type = if let Schema::AnyOf { any_of, .. } = prop_schema {
                    // First check if this should be a dynamic JSON pattern
                    if self.should_use_dynamic_json(prop_schema) {
                        // This is a dynamic JSON pattern, use serde_json::Value directly
                        SchemaType::Primitive {
                            rust_type: "serde_json::Value".to_string(),
                        }
                    } else {
                        // This is an anyOf union in a property - create a named union type
                        // Use the current schema name as context to make the union name unique
                        let context_name = self
                            .current_schema_name
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string());

                        // Generate a name based on both the schema and property name
                        let prop_pascal = self.to_pascal_case(prop_name);
                        let union_type_name = format!("{context_name}{prop_pascal}");

                        // Analyze the union
                        let union_schema_type = self.analyze_anyof_union(
                            any_of,
                            prop_schema.discriminator(),
                            dependencies,
                            &union_type_name,
                        )?;

                        // Store the union as a named schema
                        self.resolved_cache.insert(
                            union_type_name.clone(),
                            AnalyzedSchema {
                                name: union_type_name.clone(),
                                original: serde_json::to_value(prop_schema).unwrap_or(Value::Null),
                                schema_type: union_schema_type,
                                dependencies: HashSet::new(),
                                nullable: false,
                                description: prop_schema.details().description.clone(),
                                default: None,
                            },
                        );

                        // Return a reference to the named union type
                        dependencies.insert(union_type_name.clone());
                        SchemaType::Reference {
                            target: union_type_name,
                        }
                    }
                } else if let Schema::OneOf {
                    one_of,
                    discriminator,
                    ..
                } = prop_schema
                {
                    // Handle oneOf discriminated unions in properties
                    // Generate a name based on the property name
                    let context_name = self
                        .current_schema_name
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string());
                    let prop_pascal = self.to_pascal_case(prop_name);
                    let union_type_name = format!("{context_name}{prop_pascal}");

                    // Analyze the discriminated union
                    let union_schema_type = self.analyze_oneof_union(
                        one_of,
                        discriminator.as_ref(),
                        Some(&union_type_name),
                        dependencies,
                    )?;

                    // Store the union as a named schema
                    self.resolved_cache.insert(
                        union_type_name.clone(),
                        AnalyzedSchema {
                            name: union_type_name.clone(),
                            original: serde_json::to_value(prop_schema).unwrap_or(Value::Null),
                            schema_type: union_schema_type,
                            dependencies: HashSet::new(),
                            nullable: false,
                            description: prop_schema.details().description.clone(),
                            default: None,
                        },
                    );

                    // Return a reference to the named union type
                    dependencies.insert(union_type_name.clone());
                    SchemaType::Reference {
                        target: union_type_name,
                    }
                } else {
                    // Regular property schema analysis - pass property name for context
                    self.analyze_property_schema_with_context(
                        prop_schema,
                        Some(prop_name),
                        dependencies,
                    )?
                };

                let prop_details = prop_schema.details();
                // Check for both explicit nullable and anyOf nullable patterns
                let prop_nullable = prop_details.is_nullable() || prop_schema.is_nullable_pattern();
                let prop_description = prop_details.description.clone();
                let prop_default = prop_details.default.clone();

                property_info.insert(
                    prop_name.clone(),
                    PropertyInfo {
                        schema_type: prop_type,
                        nullable: prop_nullable,
                        description: prop_description,
                        default: prop_default,
                        serde_attrs: Vec::new(),
                    },
                );
            }
        }

        // Check additionalProperties setting
        let additional_properties = match &details.additional_properties {
            Some(crate::openapi::AdditionalProperties::Boolean(true)) => true,
            Some(crate::openapi::AdditionalProperties::Boolean(false)) => false,
            Some(crate::openapi::AdditionalProperties::Schema(_)) => {
                // For now, treat schema-based additionalProperties as true
                // TODO: Could analyze the schema to determine the value type
                true
            }
            None => false, // Default is false if not specified
        };

        Ok(SchemaType::Object {
            properties: property_info,
            required,
            additional_properties,
        })
    }

    fn analyze_property_schema_with_context(
        &mut self,
        schema: &Schema,
        property_name: Option<&str>,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        if let Some(ref_str) = self.get_any_reference(schema) {
            let target = if ref_str == "#" {
                // $recursiveRef: "#" - need to find the schema with $recursiveAnchor: true
                self.find_recursive_anchor_schema()
                    .unwrap_or_else(|| "UnknownRecursive".to_string())
            } else {
                self.extract_schema_name(ref_str)
                    .ok_or_else(|| GeneratorError::UnresolvedReference(ref_str.to_string()))?
                    .to_string()
            };
            dependencies.insert(target.clone());
            return Ok(SchemaType::Reference { target });
        }

        if let Some(schema_type) = schema.schema_type() {
            match schema_type {
                OpenApiSchemaType::String => {
                    // Check if this string type has enum values
                    if let Some(enum_values) = schema.details().string_enum_values() {
                        // This is an inline enum in a property - create a named enum type
                        // Use the current schema name as context to make the enum name unique
                        let context_name = self
                            .current_schema_name
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string());

                        // Generate a unique name based on both the schema and property context
                        let enum_type_name = if let Some(prop_name) = property_name {
                            // We have property name context - use it for a unique name
                            let prop_pascal = self.to_pascal_case(prop_name);
                            format!("{context_name}{prop_pascal}")
                        } else {
                            // No property name context - generate a unique name using enum values
                            // Use the first enum value to help make the name unique
                            let suffix = if !enum_values.is_empty() {
                                let first_value = self.to_pascal_case(&enum_values[0]);
                                format!("{first_value}Enum")
                            } else {
                                "StringEnum".to_string()
                            };
                            format!("{context_name}{suffix}")
                        };

                        // Check if this exact enum type was already created (for deduplication)
                        // Only reuse if the enum values are exactly the same
                        let should_create_new = !self
                            .resolved_cache
                            .get(&enum_type_name)
                            .map(|existing| {
                                if let SchemaType::StringEnum {
                                    values: existing_values,
                                } = &existing.schema_type
                                {
                                    existing_values == &enum_values
                                } else {
                                    false
                                }
                            })
                            .unwrap_or(false);

                        if should_create_new {
                            // Store the enum as a named schema
                            self.resolved_cache.insert(
                                enum_type_name.clone(),
                                AnalyzedSchema {
                                    name: enum_type_name.clone(),
                                    original: serde_json::to_value(schema).unwrap_or(Value::Null),
                                    schema_type: SchemaType::StringEnum {
                                        values: enum_values,
                                    },
                                    dependencies: HashSet::new(),
                                    nullable: false,
                                    description: schema.details().description.clone(),
                                    default: schema.details().default.clone(),
                                },
                            );
                        }

                        // Return a reference to the named enum type
                        dependencies.insert(enum_type_name.clone());
                        return Ok(SchemaType::Reference {
                            target: enum_type_name,
                        });
                    } else {
                        return Ok(SchemaType::Primitive {
                            rust_type: "String".to_string(),
                        });
                    }
                }
                OpenApiSchemaType::Integer | OpenApiSchemaType::Number => {
                    let details = schema.details();
                    let rust_type = self.get_number_rust_type(schema_type.clone(), details);
                    return Ok(SchemaType::Primitive { rust_type });
                }
                OpenApiSchemaType::Boolean => {
                    return Ok(SchemaType::Primitive {
                        rust_type: "bool".to_string(),
                    });
                }
                OpenApiSchemaType::Array => {
                    // Analyze array property with context
                    let context_name = if let Some(prop_name) = property_name {
                        // Use property name for context
                        let prop_pascal = self.to_pascal_case(prop_name);
                        format!(
                            "{}{}",
                            self.current_schema_name.as_deref().unwrap_or("Unknown"),
                            prop_pascal
                        )
                    } else {
                        // Fallback to generic name
                        "ArrayItem".to_string()
                    };
                    return self.analyze_array_schema(schema, &context_name, dependencies);
                }
                OpenApiSchemaType::Object => {
                    // Check if this is a dynamic JSON object
                    if self.should_use_dynamic_json(schema) {
                        return Ok(SchemaType::Primitive {
                            rust_type: "serde_json::Value".to_string(),
                        });
                    }
                    // Inline object in property - create a named schema for it
                    let object_type_name = if let Some(prop_name) = property_name {
                        // Use property name for context
                        let prop_pascal = self.to_pascal_case(prop_name);
                        format!(
                            "{}{}",
                            self.current_schema_name.as_deref().unwrap_or("Unknown"),
                            prop_pascal
                        )
                    } else {
                        // Fallback to generic name
                        format!(
                            "{}Object",
                            self.current_schema_name.as_deref().unwrap_or("Unknown")
                        )
                    };

                    // Analyze the object schema
                    let object_type = self.analyze_object_schema(schema, dependencies)?;

                    // Create an analyzed schema for the inline object
                    let inline_schema = AnalyzedSchema {
                        name: object_type_name.clone(),
                        original: serde_json::to_value(schema).unwrap_or(Value::Null),
                        schema_type: object_type,
                        dependencies: dependencies.clone(),
                        nullable: false,
                        description: schema.details().description.clone(),
                        default: None,
                    };

                    // Add the inline object as a named schema
                    self.resolved_cache
                        .insert(object_type_name.clone(), inline_schema);
                    dependencies.insert(object_type_name.clone());

                    // Return a reference to the named schema
                    return Ok(SchemaType::Reference {
                        target: object_type_name,
                    });
                }
                _ => {
                    return Ok(SchemaType::Primitive {
                        rust_type: "serde_json::Value".to_string(),
                    });
                }
            }
        }

        // Handle nullable patterns
        if schema.is_nullable_pattern() {
            if let Some(non_null) = schema.non_null_variant() {
                return self.analyze_property_schema_with_context(
                    non_null,
                    property_name,
                    dependencies,
                );
            }
        }

        // Check if this should be dynamic JSON before further analysis
        if self.should_use_dynamic_json(schema) {
            return Ok(SchemaType::Primitive {
                rust_type: "serde_json::Value".to_string(),
            });
        }

        // Handle allOf composition patterns
        if let Schema::AllOf { all_of, .. } = schema {
            return self.analyze_allof_composition(all_of, dependencies);
        }

        // Handle union patterns (anyOf/oneOf) that weren't caught earlier
        if let Some(variants) = schema.union_variants() {
            match variants.len().cmp(&1) {
                std::cmp::Ordering::Equal => {
                    // Single variant - analyze it directly
                    return self.analyze_property_schema_with_context(
                        &variants[0],
                        property_name,
                        dependencies,
                    );
                }
                std::cmp::Ordering::Greater => {
                    // Multiple variants - try to analyze as a union
                    // Generate a context-aware name for the union type
                    let union_name = if let Some(prop_name) = property_name {
                        // We have property context - create a proper union name
                        let prop_pascal = self.to_pascal_case(prop_name);
                        format!(
                            "{}{}",
                            self.current_schema_name.as_deref().unwrap_or(""),
                            prop_pascal
                        )
                    } else {
                        "UnionType".to_string()
                    };

                    // Check if this is a oneOf or anyOf
                    if let Schema::OneOf {
                        one_of,
                        discriminator,
                        ..
                    } = schema
                    {
                        // This is a oneOf - analyze it properly with potential discriminator
                        let oneof_result = self.analyze_oneof_union(
                            one_of,
                            discriminator.as_ref(),
                            Some(&union_name),
                            dependencies,
                        )?;

                        // If we got a union type (not discriminated), we need to store it as a named type
                        if let SchemaType::Union {
                            variants: _union_variants,
                        } = &oneof_result
                        {
                            // Store the union as a named type in resolved_cache
                            self.resolved_cache.insert(
                                union_name.clone(),
                                AnalyzedSchema {
                                    name: union_name.clone(),
                                    original: serde_json::to_value(schema).unwrap_or(Value::Null),
                                    schema_type: oneof_result.clone(),
                                    dependencies: dependencies.clone(),
                                    nullable: false,
                                    description: schema.details().description.clone(),
                                    default: None,
                                },
                            );

                            // Return a reference to the named union type
                            dependencies.insert(union_name.clone());
                            return Ok(SchemaType::Reference { target: union_name });
                        }

                        return Ok(oneof_result);
                    } else if let Schema::AnyOf {
                        any_of,
                        discriminator,
                        ..
                    } = schema
                    {
                        // This is anyOf - use existing logic with discriminator support
                        let union_analysis = self.analyze_anyof_union(
                            any_of,
                            discriminator.as_ref(),
                            dependencies,
                            &union_name,
                        )?;
                        return Ok(union_analysis);
                    } else {
                        // This shouldn't happen, but handle gracefully
                        // Create a simple union from variants
                        let mut union_variants = Vec::new();
                        for variant in variants {
                            if let Some(ref_str) = variant.reference() {
                                if let Some(target) = self.extract_schema_name(ref_str) {
                                    dependencies.insert(target.to_string());
                                    union_variants.push(SchemaRef {
                                        target: target.to_string(),
                                        nullable: false,
                                    });
                                }
                            }
                        }
                        return Ok(SchemaType::Union {
                            variants: union_variants,
                        });
                    }
                }
                std::cmp::Ordering::Less => {}
            }
        }

        // Handle untyped schemas by trying to infer from structure
        if let Some(inferred_type) = schema.inferred_type() {
            match inferred_type {
                OpenApiSchemaType::Object => {
                    // Double-check for dynamic JSON pattern even for inferred objects
                    if self.should_use_dynamic_json(schema) {
                        return Ok(SchemaType::Primitive {
                            rust_type: "serde_json::Value".to_string(),
                        });
                    }
                    return self.analyze_object_schema(schema, dependencies);
                }
                OpenApiSchemaType::Array => {
                    let context_name = if let Some(prop_name) = property_name {
                        // Use property name for context
                        let prop_pascal = self.to_pascal_case(prop_name);
                        format!(
                            "{}{}",
                            self.current_schema_name.as_deref().unwrap_or("Unknown"),
                            prop_pascal
                        )
                    } else {
                        // Fallback to generic name
                        "ArrayItem".to_string()
                    };
                    return self.analyze_array_schema(schema, &context_name, dependencies);
                }
                OpenApiSchemaType::String => {
                    if let Some(enum_values) = schema.details().string_enum_values() {
                        return Ok(SchemaType::StringEnum {
                            values: enum_values,
                        });
                    } else {
                        return Ok(SchemaType::Primitive {
                            rust_type: "String".to_string(),
                        });
                    }
                }
                _ => {
                    // Handle other inferred types
                    let rust_type = self.openapi_type_to_rust_type(inferred_type, schema.details());
                    return Ok(SchemaType::Primitive { rust_type });
                }
            }
        }

        Ok(SchemaType::Primitive {
            rust_type: "serde_json::Value".to_string(),
        })
    }

    fn analyze_allof_composition(
        &mut self,
        all_of_schemas: &[Schema],
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        // Special case: if allOf contains only a single reference, treat it as a direct type alias
        // This handles patterns like: "allOf": [{"$ref": "#/components/schemas/Usage"}]
        if all_of_schemas.len() == 1 {
            if let Schema::Reference { reference, .. } = &all_of_schemas[0] {
                if let Some(target) = self.extract_schema_name(reference) {
                    dependencies.insert(target.to_string());
                    return Ok(SchemaType::Reference {
                        target: target.to_string(),
                    });
                }
            }
        }

        // AllOf represents schema composition - merge all schemas into one
        let mut merged_properties = BTreeMap::new();
        let mut merged_required = HashSet::new();
        let mut descriptions = Vec::new();

        // Save the current schema context to restore it when analyzing properties
        let current_context = self.current_schema_name.clone();

        for schema in all_of_schemas {
            match schema {
                Schema::Reference { reference, .. } => {
                    // Add dependency on referenced schema
                    if let Some(target) = self.extract_schema_name(reference) {
                        dependencies.insert(target.to_string());

                        // First ensure the referenced schema is analyzed
                        let analyzed_ref = self.analyze_schema(target)?;

                        // Now merge the analyzed schema's properties
                        match &analyzed_ref.schema_type {
                            SchemaType::Object {
                                properties,
                                required,
                                ..
                            } => {
                                // Merge properties from the analyzed schema
                                for (prop_name, prop_info) in properties {
                                    merged_properties.insert(prop_name.clone(), prop_info.clone());
                                }
                                // Merge required fields
                                for req in required {
                                    merged_required.insert(req.clone());
                                }
                            }
                            _ => {
                                // If the referenced schema is not an object, fall back to raw merge
                                if let Some(ref_schema) = self.schemas.get(target).cloned() {
                                    self.merge_schema_into_properties(
                                        &ref_schema,
                                        &mut merged_properties,
                                        &mut merged_required,
                                        dependencies,
                                    )?;
                                }
                            }
                        }
                    }
                }
                Schema::Typed {
                    schema_type: OpenApiSchemaType::Object,
                    ..
                }
                | Schema::Untyped { .. } => {
                    // Restore the original context when analyzing inline properties
                    let saved_context = self.current_schema_name.clone();
                    self.current_schema_name = current_context.clone();

                    // Merge object properties directly
                    self.merge_schema_into_properties(
                        schema,
                        &mut merged_properties,
                        &mut merged_required,
                        dependencies,
                    )?;

                    // Restore the previous context
                    self.current_schema_name = saved_context;
                }
                _ => {
                    // For non-object typed schemas in allOf, try to merge them as well
                    // This handles cases like allOf with enum or string constraints
                    self.merge_schema_into_properties(
                        schema,
                        &mut merged_properties,
                        &mut merged_required,
                        dependencies,
                    )?;
                }
            }

            // Collect descriptions
            if let Some(desc) = &schema.details().description {
                descriptions.push(desc.clone());
            }
        }

        // If we successfully merged properties, return an object
        if !merged_properties.is_empty() {
            Ok(SchemaType::Object {
                properties: merged_properties,
                required: merged_required,
                additional_properties: false,
            })
        } else {
            // Fall back to composition if we couldn't merge
            Ok(SchemaType::Composition {
                schemas: all_of_schemas
                    .iter()
                    .filter_map(|s| {
                        if let Some(ref_str) = s.reference() {
                            if let Some(target) = self.extract_schema_name(ref_str) {
                                dependencies.insert(target.to_string());
                                Some(SchemaRef {
                                    target: target.to_string(),
                                    nullable: false,
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect(),
            })
        }
    }

    fn merge_schema_into_properties(
        &mut self,
        schema: &Schema,
        merged_properties: &mut BTreeMap<String, PropertyInfo>,
        merged_required: &mut HashSet<String>,
        dependencies: &mut HashSet<String>,
    ) -> Result<()> {
        let details = schema.details();

        // Merge properties
        if let Some(properties) = &details.properties {
            for (prop_name, prop_schema) in properties {
                let prop_type = self.analyze_property_schema_with_context(
                    prop_schema,
                    Some(prop_name),
                    dependencies,
                )?;
                let prop_details = prop_schema.details();

                merged_properties.insert(
                    prop_name.clone(),
                    PropertyInfo {
                        schema_type: prop_type,
                        nullable: prop_details.is_nullable(),
                        description: prop_details.description.clone(),
                        default: prop_details.default.clone(),
                        serde_attrs: Vec::new(),
                    },
                );
            }
        }

        // Merge required fields
        if let Some(required) = &details.required {
            for field in required {
                merged_required.insert(field.clone());
            }
        }

        Ok(())
    }

    fn analyze_oneof_union(
        &mut self,
        one_of_schemas: &[Schema],
        discriminator: Option<&crate::openapi::Discriminator>,
        parent_name: Option<&str>,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        // If there's no discriminator, we should create an untagged union
        if discriminator.is_none() {
            // Handle untagged unions (oneOf without discriminator)
            return self.analyze_untagged_oneof_union(one_of_schemas, parent_name, dependencies);
        }

        // This is a discriminated union
        let discriminator_field = discriminator
            .ok_or_else(|| {
                GeneratorError::InvalidDiscriminator(
                    "expected discriminator after guard check".to_string(),
                )
            })?
            .property_name
            .clone();

        let mut variants = Vec::new();
        let mut used_variant_names = std::collections::HashSet::new();

        for variant_schema in one_of_schemas {
            // Check if this is a direct reference, recursive reference, or an allOf wrapper with a reference
            let ref_info = if let Some(ref_str) = variant_schema.reference() {
                Some((ref_str, false))
            } else if let Some(recursive_ref) = variant_schema.recursive_reference() {
                Some((recursive_ref, true))
            } else if let Schema::AllOf { all_of, .. } = variant_schema {
                // Check if this is an allOf with a single reference
                if all_of.len() == 1 {
                    if let Some(ref_str) = all_of[0].reference() {
                        Some((ref_str, false))
                    } else {
                        all_of[0]
                            .recursive_reference()
                            .map(|recursive_ref| (recursive_ref, true))
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some((ref_str, is_recursive)) = ref_info {
                let schema_name = if is_recursive && ref_str == "#" {
                    // Handle recursive reference to the schema with recursiveAnchor
                    self.find_recursive_anchor_schema()
                        .or_else(|| self.current_schema_name.clone())
                        .unwrap_or_else(|| "CompoundFilter".to_string())
                } else {
                    self.extract_schema_name(ref_str)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "UnknownRef".to_string())
                };

                if !schema_name.is_empty() {
                    dependencies.insert(schema_name.clone());

                    // Determine discriminator value with priority order:
                    // 1. Explicit mapping in discriminator
                    // 2. Extract from referenced schema
                    // 3. Generate from schema name
                    let discriminator_value = if let Some(disc) = discriminator {
                        if let Some(mappings) = &disc.mapping {
                            // Find the mapping key that points to this schema reference
                            // Mapping format is: "discriminator_value" -> "#/components/schemas/SchemaName"
                            mappings
                                .iter()
                                .find(|(_, target_ref)| {
                                    // Check if this mapping target matches our reference
                                    target_ref.as_str() == ref_str
                                        || self
                                            .extract_schema_name(target_ref)
                                            .map(|s| s.to_string())
                                            == Some(schema_name.clone())
                                })
                                .map(|(key, _)| key.clone())
                                .unwrap_or_else(|| {
                                    self.fallback_discriminator_value_for_field(
                                        &schema_name,
                                        &discriminator_field,
                                    )
                                })
                        } else {
                            self.fallback_discriminator_value_for_field(
                                &schema_name,
                                &discriminator_field,
                            )
                        }
                    } else {
                        self.fallback_discriminator_value_for_field(
                            &schema_name,
                            &discriminator_field,
                        )
                    };

                    // Generate Rust-friendly variant name and ensure uniqueness
                    let base_name = self.to_rust_variant_name(&schema_name);
                    let rust_name =
                        self.ensure_unique_variant_name(base_name, &mut used_variant_names);

                    // Use the discriminator value as-is from the schema
                    let final_discriminator_value = discriminator_value;

                    variants.push(UnionVariant {
                        rust_name,
                        type_name: schema_name,
                        discriminator_value: final_discriminator_value,
                        schema_ref: ref_str.to_string(),
                    });
                }
            } else {
                // Handle inline schemas in oneOf
                let variant_index = variants.len();
                let inline_type_name =
                    self.generate_inline_type_name(variant_schema, variant_index);

                // Try to extract discriminator value from inline schema
                let discriminator_value = if let Some(disc) = discriminator {
                    if let Some(mappings) = &disc.mapping {
                        // Look for mapping that points to this inline variant by index
                        mappings
                            .iter()
                            .find(|(_, target_ref)| {
                                target_ref.contains(&format!("variant_{variant_index}"))
                            })
                            .map(|(key, _)| key.clone())
                            .unwrap_or_else(|| {
                                self.extract_inline_discriminator_value(
                                    variant_schema,
                                    &discriminator_field,
                                    variant_index,
                                )
                            })
                    } else {
                        self.extract_inline_discriminator_value(
                            variant_schema,
                            &discriminator_field,
                            variant_index,
                        )
                    }
                } else {
                    self.extract_inline_discriminator_value(
                        variant_schema,
                        &discriminator_field,
                        variant_index,
                    )
                };

                // Generate Rust-friendly variant name based on discriminator or fallback to generic
                let base_name = if discriminator_value.starts_with("variant_") {
                    format!("Variant{variant_index}")
                } else {
                    // Convert discriminator value to a meaningful Rust variant name
                    let clean_name = self.discriminator_to_variant_name(&discriminator_value);
                    self.to_rust_variant_name(&clean_name)
                };
                let rust_name = self.ensure_unique_variant_name(base_name, &mut used_variant_names);

                // Use the discriminator value as-is from the schema
                let final_discriminator_value = discriminator_value;

                variants.push(UnionVariant {
                    rust_name,
                    type_name: inline_type_name.clone(),
                    discriminator_value: final_discriminator_value,
                    schema_ref: format!("inline_{variant_index}"),
                });

                // Store inline schema for later analysis and generation
                self.add_inline_schema(&inline_type_name, variant_schema, dependencies)?;
            }
        }

        if variants.is_empty() {
            // If we couldn't create a discriminated union, fall back to an untagged union
            // This handles cases where oneOf contains references or inline schemas without proper discriminators
            let mut union_variants = Vec::new();

            for (variant_index, variant_schema) in one_of_schemas.iter().enumerate() {
                // First check if it's a reference or recursive reference
                if let Some(ref_str) = variant_schema.reference() {
                    if let Some(schema_name) = self.extract_schema_name(ref_str) {
                        dependencies.insert(schema_name.to_string());
                        union_variants.push(SchemaRef {
                            target: schema_name.to_string(),
                            nullable: false,
                        });
                    }
                } else if let Some(recursive_ref) = variant_schema.recursive_reference() {
                    let schema_name = if recursive_ref == "#" {
                        // Handle recursive reference to the schema with recursiveAnchor
                        self.find_recursive_anchor_schema()
                            .or_else(|| self.current_schema_name.clone())
                            .unwrap_or_else(|| "CompoundFilter".to_string())
                    } else {
                        self.extract_schema_name(recursive_ref)
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "RecursiveType".to_string())
                    };
                    dependencies.insert(schema_name.clone());
                    union_variants.push(SchemaRef {
                        target: schema_name,
                        nullable: false,
                    });
                } else {
                    // Handle inline schemas by creating type aliases or using primitive types directly
                    let context = parent_name.unwrap_or("Union");
                    let inline_name = self.generate_context_aware_name(
                        context,
                        "InlineVariant",
                        variant_index,
                        Some(variant_schema),
                    );
                    let analyzed = self.analyze_schema_value(variant_schema, &inline_name)?;
                    let variant_type = analyzed.schema_type;

                    // Add dependencies from the analyzed schema
                    for dep in &analyzed.dependencies {
                        dependencies.insert(dep.clone());
                    }

                    match &variant_type {
                        // For primitive types, we can use them directly in the union
                        SchemaType::Primitive { rust_type } => {
                            union_variants.push(SchemaRef {
                                target: rust_type.clone(),
                                nullable: false,
                            });
                        }
                        // For arrays, check if we can determine the item type
                        SchemaType::Array { item_type } => {
                            match item_type.as_ref() {
                                SchemaType::Primitive { rust_type } => {
                                    let type_name = format!("Vec<{rust_type}>");
                                    union_variants.push(SchemaRef {
                                        target: type_name,
                                        nullable: false,
                                    });
                                }
                                SchemaType::Reference { target } => {
                                    let type_name = format!("Vec<{target}>");
                                    union_variants.push(SchemaRef {
                                        target: type_name,
                                        nullable: false,
                                    });
                                }
                                _ => {
                                    // For other array types, create an inline type
                                    let context = parent_name.unwrap_or("Inline");
                                    let inline_type_name = self.generate_context_aware_name(
                                        context,
                                        "Variant",
                                        variant_index,
                                        None,
                                    );
                                    self.add_inline_schema(
                                        &inline_type_name,
                                        variant_schema,
                                        dependencies,
                                    )?;
                                    union_variants.push(SchemaRef {
                                        target: inline_type_name,
                                        nullable: false,
                                    });
                                }
                            }
                        }
                        // For reference types, use the reference target directly
                        SchemaType::Reference { target } => {
                            union_variants.push(SchemaRef {
                                target: target.clone(),
                                nullable: false,
                            });
                        }
                        // For other complex types, create an inline type
                        _ => {
                            let inline_type_name = format!(
                                "{}Variant{}",
                                parent_name.unwrap_or("Inline"),
                                variant_index + 1
                            );
                            self.add_inline_schema(
                                &inline_type_name,
                                variant_schema,
                                dependencies,
                            )?;
                            union_variants.push(SchemaRef {
                                target: inline_type_name,
                                nullable: false,
                            });
                        }
                    }
                }
            }

            if !union_variants.is_empty() {
                return Ok(SchemaType::Union {
                    variants: union_variants,
                });
            }

            // Only fall back to serde_json::Value if we truly can't analyze the union
            return Ok(SchemaType::Primitive {
                rust_type: "serde_json::Value".to_string(),
            });
        }

        Ok(SchemaType::DiscriminatedUnion {
            discriminator_field,
            variants,
        })
    }

    fn analyze_untagged_oneof_union(
        &mut self,
        one_of_schemas: &[Schema],
        parent_name: Option<&str>,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        let mut union_variants = Vec::new();

        for (variant_index, variant_schema) in one_of_schemas.iter().enumerate() {
            // First check if it's a reference or recursive reference
            if let Some(ref_str) = variant_schema.reference() {
                if let Some(schema_name) = self.extract_schema_name(ref_str) {
                    dependencies.insert(schema_name.to_string());
                    union_variants.push(SchemaRef {
                        target: schema_name.to_string(),
                        nullable: false,
                    });
                }
            } else if let Some(recursive_ref) = variant_schema.recursive_reference() {
                let schema_name = if recursive_ref == "#" {
                    // Handle recursive reference to the schema with recursiveAnchor
                    self.find_recursive_anchor_schema()
                        .or_else(|| self.current_schema_name.clone())
                        .unwrap_or_else(|| "CompoundFilter".to_string())
                } else {
                    self.extract_schema_name(recursive_ref)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "RecursiveType".to_string())
                };
                dependencies.insert(schema_name.clone());
                union_variants.push(SchemaRef {
                    target: schema_name,
                    nullable: false,
                });
            } else {
                // Handle inline schemas by creating type aliases or using primitive types directly
                let context = parent_name.unwrap_or("Union");
                let inline_name = self.generate_context_aware_name(
                    context,
                    "InlineVariant",
                    variant_index,
                    Some(variant_schema),
                );
                let analyzed = self.analyze_schema_value(variant_schema, &inline_name)?;
                let variant_type = analyzed.schema_type;

                // Add dependencies from the analyzed schema
                for dep in &analyzed.dependencies {
                    dependencies.insert(dep.clone());
                }

                match &variant_type {
                    // For primitive types, we can use them directly in the union
                    SchemaType::Primitive { rust_type } => {
                        union_variants.push(SchemaRef {
                            target: rust_type.clone(),
                            nullable: false,
                        });
                    }
                    // For arrays, check if we can determine the item type
                    SchemaType::Array { item_type } => {
                        match item_type.as_ref() {
                            SchemaType::Primitive { rust_type } => {
                                let type_name = format!("Vec<{rust_type}>");
                                union_variants.push(SchemaRef {
                                    target: type_name,
                                    nullable: false,
                                });
                            }
                            SchemaType::Reference { target } => {
                                let type_name = format!("Vec<{target}>");
                                union_variants.push(SchemaRef {
                                    target: type_name,
                                    nullable: false,
                                });
                            }
                            // Handle arrays of arrays (e.g., Vec<Vec<i64>>)
                            SchemaType::Array {
                                item_type: inner_item_type,
                            } => {
                                match inner_item_type.as_ref() {
                                    SchemaType::Primitive { rust_type } => {
                                        let type_name = format!("Vec<Vec<{rust_type}>>");
                                        union_variants.push(SchemaRef {
                                            target: type_name,
                                            nullable: false,
                                        });
                                    }
                                    SchemaType::Reference { target } => {
                                        let type_name = format!("Vec<Vec<{target}>>");
                                        union_variants.push(SchemaRef {
                                            target: type_name,
                                            nullable: false,
                                        });
                                    }
                                    _ => {
                                        // For deeper nesting, create an inline type
                                        let context = parent_name.unwrap_or("Inline");
                                        let inline_type_name = self.generate_context_aware_name(
                                            context,
                                            "Variant",
                                            variant_index,
                                            None,
                                        );
                                        self.add_inline_schema(
                                            &inline_type_name,
                                            variant_schema,
                                            dependencies,
                                        )?;
                                        union_variants.push(SchemaRef {
                                            target: inline_type_name,
                                            nullable: false,
                                        });
                                    }
                                }
                            }
                            _ => {
                                // For other array types, create an inline type
                                let context = parent_name.unwrap_or("Inline");
                                let inline_type_name = self.generate_context_aware_name(
                                    context,
                                    "Variant",
                                    variant_index,
                                    None,
                                );
                                self.add_inline_schema(
                                    &inline_type_name,
                                    variant_schema,
                                    dependencies,
                                )?;
                                union_variants.push(SchemaRef {
                                    target: inline_type_name,
                                    nullable: false,
                                });
                            }
                        }
                    }
                    // For reference types, use the reference target directly
                    SchemaType::Reference { target } => {
                        union_variants.push(SchemaRef {
                            target: target.clone(),
                            nullable: false,
                        });
                    }
                    // For other complex types, create an inline type
                    _ => {
                        let context = parent_name.unwrap_or("Inline");
                        let inline_type_name = self.generate_context_aware_name(
                            context,
                            "Variant",
                            variant_index,
                            None,
                        );
                        self.add_inline_schema(&inline_type_name, variant_schema, dependencies)?;
                        union_variants.push(SchemaRef {
                            target: inline_type_name,
                            nullable: false,
                        });
                    }
                }
            }
        }

        if !union_variants.is_empty() {
            return Ok(SchemaType::Union {
                variants: union_variants,
            });
        }

        // Only fall back to serde_json::Value if we truly can't analyze the union
        Ok(SchemaType::Primitive {
            rust_type: "serde_json::Value".to_string(),
        })
    }

    fn add_inline_schema(
        &mut self,
        type_name: &str,
        schema: &Schema,
        dependencies: &mut HashSet<String>,
    ) -> Result<()> {
        // For primitive types, we need to ensure they are stored as type aliases
        if let Some(schema_type) = schema.schema_type() {
            match schema_type {
                OpenApiSchemaType::String
                | OpenApiSchemaType::Integer
                | OpenApiSchemaType::Number
                | OpenApiSchemaType::Boolean => {
                    let rust_type =
                        self.openapi_type_to_rust_type(schema_type.clone(), schema.details());

                    // Store as a type alias
                    self.resolved_cache.insert(
                        type_name.to_string(),
                        AnalyzedSchema {
                            name: type_name.to_string(),
                            original: serde_json::to_value(schema).unwrap_or(Value::Null),
                            schema_type: SchemaType::Primitive { rust_type },
                            dependencies: HashSet::new(),
                            nullable: false,
                            description: schema.details().description.clone(),
                            default: None,
                        },
                    );
                    return Ok(());
                }
                _ => {}
            }
        }

        // For non-primitive types, analyze the inline schema and add it to our collection
        // Set current_schema_name so nested inline properties (enums, unions, objects)
        // get named with the correct parent context instead of inheriting a stale name
        let previous_schema_name = self.current_schema_name.take();
        self.current_schema_name = Some(type_name.to_string());
        let analyzed = self.analyze_schema_value(schema, type_name)?;
        self.current_schema_name = previous_schema_name;

        // Add to resolved cache so it can be generated
        self.resolved_cache.insert(type_name.to_string(), analyzed);

        // Add dependencies
        if let Some(cached) = self.resolved_cache.get(type_name) {
            for dep in &cached.dependencies {
                dependencies.insert(dep.clone());
            }
        }

        Ok(())
    }

    fn extract_inline_discriminator_value(
        &self,
        schema: &Schema,
        discriminator_field: &str,
        variant_index: usize,
    ) -> String {
        // Try to extract discriminator value from inline schema properties
        if let Some(properties) = &schema.details().properties {
            if let Some(discriminator_prop) = properties.get(discriminator_field) {
                // Check for enum with single value
                if let Some(enum_values) = &discriminator_prop.details().enum_values {
                    if enum_values.len() == 1 {
                        if let Some(value) = enum_values[0].as_str() {
                            return value.to_string();
                        }
                    }
                }
                // Check for const value in extra fields
                if let Some(const_value) = discriminator_prop.details().extra.get("const") {
                    if let Some(value) = const_value.as_str() {
                        return value.to_string();
                    }
                }
                // Check for const value in the discriminator_prop.details().const_value
                if let Some(const_value) = &discriminator_prop.details().const_value {
                    if let Some(value) = const_value.as_str() {
                        return value.to_string();
                    }
                }
            }
        }

        // Try to infer from schema structure and properties
        if let Some(inferred_name) = self.infer_variant_name_from_structure(schema, variant_index) {
            return inferred_name;
        }

        // Fall back to generic variant name
        format!("variant_{variant_index}")
    }

    fn infer_variant_name_from_structure(
        &self,
        schema: &Schema,
        _variant_index: usize,
    ) -> Option<String> {
        let details = schema.details();

        // Strategy 1: Look for unique property combinations that suggest the variant type
        if let Some(properties) = &details.properties {
            // Common patterns for content blocks
            if properties.contains_key("text") && properties.len() <= 3 {
                return Some("text".to_string());
            }
            if properties.contains_key("image") || properties.contains_key("source") {
                return Some("image".to_string());
            }
            if properties.contains_key("document") {
                return Some("document".to_string());
            }
            if properties.contains_key("tool_use_id") || properties.contains_key("tool_result") {
                return Some("tool_result".to_string());
            }
            if properties.contains_key("content") && properties.contains_key("is_error") {
                return Some("tool_result".to_string());
            }
            if properties.contains_key("partial_json") {
                return Some("partial_json".to_string());
            }

            // Strategy 2: Look for properties that hint at the variant purpose
            let property_names: Vec<&String> = properties.keys().collect();

            // Try to find the most descriptive property name
            for prop_name in &property_names {
                if prop_name.contains("result") {
                    return Some("result".to_string());
                }
                if prop_name.contains("error") {
                    return Some("error".to_string());
                }
                if prop_name.contains("content") && property_names.len() <= 2 {
                    return Some("content".to_string());
                }
            }

            // Strategy 3: Use the most significant unique property
            let significant_props = property_names
                .iter()
                .filter(|&name| !["type", "id", "cache_control"].contains(&name.as_str()))
                .collect::<Vec<_>>();

            if significant_props.len() == 1 {
                return Some((*significant_props[0]).clone());
            }
        }

        // Strategy 4: Look at description for hints
        if let Some(description) = &details.description {
            let desc_lower = description.to_lowercase();
            if desc_lower.contains("text") && desc_lower.len() < 100 {
                return Some("text".to_string());
            }
            if desc_lower.contains("image") {
                return Some("image".to_string());
            }
            if desc_lower.contains("document") {
                return Some("document".to_string());
            }
            if desc_lower.contains("tool") && desc_lower.contains("result") {
                return Some("tool_result".to_string());
            }
        }

        None
    }

    fn discriminator_to_variant_name(&self, discriminator: &str) -> String {
        // Convert discriminator values to PascalCase variant names using general rules
        if discriminator.is_empty() {
            return "Variant".to_string();
        }

        let mut result = String::new();
        let mut next_upper = true;

        for c in discriminator.chars() {
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
                '_' | '-' | '.' | ' ' | '/' | '\\' => {
                    // Word separators - next char should be uppercase
                    next_upper = true;
                }
                _ => {
                    // Other special characters - treat as word boundary
                    next_upper = true;
                }
            }
        }

        // Ensure it starts with a letter
        if result.is_empty() || result.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            result = format!("Variant{result}");
        }

        result
    }

    fn ensure_unique_variant_name(
        &self,
        base_name: String,
        used_names: &mut std::collections::HashSet<String>,
    ) -> String {
        let mut candidate = base_name.clone();
        let mut counter = 1;

        while used_names.contains(&candidate) {
            counter += 1;
            candidate = format!("{base_name}{counter}");
        }

        used_names.insert(candidate.clone());
        candidate
    }

    fn generate_inline_type_name(&self, schema: &Schema, variant_index: usize) -> String {
        // Try to generate a meaningful name for inline schemas
        if let Some(meaningful_name) = self.infer_type_name_from_structure(schema) {
            return meaningful_name;
        }

        // Fallback to context-aware name
        let context = self.current_schema_name.as_deref().unwrap_or("Inline");
        self.generate_context_aware_name(context, "Variant", variant_index, Some(schema))
    }

    fn infer_type_name_from_structure(&self, schema: &Schema) -> Option<String> {
        let details = schema.details();

        // Strategy 1: Use description if it's short and descriptive
        if let Some(description) = &details.description {
            if let Some(name_from_desc) = self.extract_type_name_from_description(description) {
                return Some(name_from_desc);
            }
        }

        // Strategy 2: Use the most significant property name as the type identifier
        if let Some(properties) = &details.properties {
            if let Some(name_from_props) = self.extract_type_name_from_properties(properties) {
                return Some(format!("{name_from_props}Block"));
            }
        }

        None
    }

    fn extract_type_name_from_description(&self, description: &str) -> Option<String> {
        // Only use descriptions that are short and likely to be type identifiers
        if description.len() > 100 || description.contains('\n') {
            return None;
        }

        // Extract the first meaningful word(s) from the description
        let words: Vec<&str> = description
            .split_whitespace()
            .take(2) // Only take first 2 words to avoid long names
            .filter(|word| {
                let w = word.to_lowercase();
                word.len() > 2
                    && ![
                        "the", "and", "for", "with", "that", "this", "are", "can", "will", "was",
                    ]
                    .contains(&w.as_str())
            })
            .collect();

        if words.is_empty() {
            return None;
        }

        // Convert to PascalCase using our existing logic
        let combined = words.join("_");
        let pascal_name = self.discriminator_to_variant_name(&combined);

        // Add suffix if it doesn't already have one
        if !pascal_name.ends_with("Content")
            && !pascal_name.ends_with("Block")
            && !pascal_name.ends_with("Type")
        {
            Some(format!("{pascal_name}Content"))
        } else {
            Some(pascal_name)
        }
    }

    fn extract_type_name_from_properties(
        &self,
        properties: &std::collections::BTreeMap<String, crate::openapi::Schema>,
    ) -> Option<String> {
        // Get property names, excluding common structural properties
        let significant_props: Vec<&String> = properties
            .keys()
            .filter(|name| !["type", "id", "cache_control"].contains(&name.as_str()))
            .collect();

        if significant_props.is_empty() {
            return None;
        }

        // Strategy 1: If there's only one significant property, use it
        if significant_props.len() == 1 {
            let prop_name = significant_props[0];
            return Some(self.discriminator_to_variant_name(prop_name));
        }

        // Strategy 2: Use the first property alphabetically for consistency
        // This provides deterministic naming without hardcoded preferences
        let mut sorted_props = significant_props.clone();
        sorted_props.sort();
        if let Some(first_prop) = sorted_props.first() {
            return Some(self.discriminator_to_variant_name(first_prop));
        }

        None
    }

    fn openapi_type_to_rust_type(
        &self,
        openapi_type: OpenApiSchemaType,
        details: &crate::openapi::SchemaDetails,
    ) -> String {
        match openapi_type {
            OpenApiSchemaType::String => "String".to_string(),
            OpenApiSchemaType::Integer => self.get_number_rust_type(openapi_type, details),
            OpenApiSchemaType::Number => self.get_number_rust_type(openapi_type, details),
            OpenApiSchemaType::Boolean => "bool".to_string(),
            OpenApiSchemaType::Array => "Vec<serde_json::Value>".to_string(), // Fallback for arrays without items
            OpenApiSchemaType::Object => "serde_json::Value".to_string(), // Fallback for untyped objects
            OpenApiSchemaType::Null => "()".to_string(),                  // Null type
        }
    }

    #[allow(dead_code)]
    fn fallback_discriminator_value(&self, schema_name: &str) -> String {
        self.fallback_discriminator_value_for_field(schema_name, "type")
    }

    fn fallback_discriminator_value_for_field(
        &self,
        schema_name: &str,
        field_name: &str,
    ) -> String {
        // Try to extract from referenced schema first
        if let Some(ref_schema) = self.schemas.get(schema_name) {
            if let Some(extracted) =
                self.extract_discriminator_value_for_field(ref_schema, field_name)
            {
                return extracted;
            }
        }

        // Fall back to generating from name
        self.generate_discriminator_value_from_name(schema_name)
    }

    fn generate_discriminator_value_from_name(&self, schema_name: &str) -> String {
        // Convert schema names like "ResponseCreatedEvent" to "response.created"
        let mut result = String::new();
        let mut chars = schema_name.chars().peekable();
        let mut first = true;

        while let Some(c) = chars.next() {
            if c.is_uppercase()
                && !first
                && chars
                    .peek()
                    .map(|&next| next.is_lowercase())
                    .unwrap_or(false)
            {
                result.push('.');
            }
            result.push(c.to_ascii_lowercase());
            first = false;
        }

        // Remove common suffixes
        if result.ends_with("event") {
            result = result[..result.len() - 5].to_string();
        }

        // Add "response." prefix if it looks like a response event
        if schema_name.starts_with("Response") && !result.starts_with("response.") {
            result = format!("response.{}", result.trim_start_matches("response"));
        }

        result
    }

    fn to_rust_variant_name(&self, schema_name: &str) -> String {
        // Convert "ResponseCreatedEvent" to "Created", "UserStatus" to "UserStatus", etc.
        let mut name = schema_name;

        // Remove common prefixes for cleaner variant names
        if name.starts_with("Response") && name.len() > 8 {
            name = &name[8..]; // Remove "Response"
        }

        // Remove common suffixes
        if name.ends_with("Event") && name.len() > 5 {
            name = &name[..name.len() - 5]; // Remove "Event"
        }

        // Trim leading and trailing underscores
        name = name.trim_matches('_');

        // Convert underscores to camel case using our existing function
        if name.is_empty() {
            schema_name.to_string()
        } else {
            // Use discriminator_to_variant_name to properly handle underscores
            self.discriminator_to_variant_name(name)
        }
    }

    fn analyze_array_schema(
        &mut self,
        schema: &Schema,
        parent_schema_name: &str,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        let details = schema.details();

        // Check if items field is present
        if let Some(items_schema) = &details.items {
            // Analyze the item type
            let item_type = match items_schema.as_ref() {
                Schema::Reference { reference, .. } => {
                    // Array of referenced types
                    let target = self
                        .extract_schema_name(reference)
                        .ok_or_else(|| GeneratorError::UnresolvedReference(reference.to_string()))?
                        .to_string();
                    dependencies.insert(target.clone());
                    SchemaType::Reference { target }
                }
                Schema::RecursiveRef { recursive_ref, .. } => {
                    // Array of recursive references
                    if recursive_ref == "#" {
                        // Self-reference to the current schema
                        let target = self
                            .find_recursive_anchor_schema()
                            .unwrap_or_else(|| parent_schema_name.to_string());
                        dependencies.insert(target.clone());
                        SchemaType::Reference { target }
                    } else {
                        let target = self
                            .extract_schema_name(recursive_ref)
                            .unwrap_or("RecursiveType")
                            .to_string();
                        dependencies.insert(target.clone());
                        SchemaType::Reference { target }
                    }
                }
                Schema::Typed { schema_type, .. } => {
                    // Array of primitive types
                    match schema_type {
                        OpenApiSchemaType::String => SchemaType::Primitive {
                            rust_type: "String".to_string(),
                        },
                        OpenApiSchemaType::Integer | OpenApiSchemaType::Number => {
                            let details = items_schema.details();
                            let rust_type = self.get_number_rust_type(schema_type.clone(), details);
                            SchemaType::Primitive { rust_type }
                        }
                        OpenApiSchemaType::Boolean => SchemaType::Primitive {
                            rust_type: "bool".to_string(),
                        },
                        OpenApiSchemaType::Object => {
                            // Inline object in array - create a named schema for it
                            let object_type_name = format!("{parent_schema_name}Item");

                            // Analyze the object schema
                            let object_type =
                                self.analyze_object_schema(items_schema, dependencies)?;

                            // Create an analyzed schema for the inline object
                            let inline_schema = AnalyzedSchema {
                                name: object_type_name.clone(),
                                original: serde_json::to_value(items_schema).unwrap_or(Value::Null),
                                schema_type: object_type,
                                dependencies: dependencies.clone(),
                                nullable: false,
                                description: items_schema.details().description.clone(),
                                default: None,
                            };

                            // Add the inline object as a named schema
                            self.resolved_cache
                                .insert(object_type_name.clone(), inline_schema);
                            dependencies.insert(object_type_name.clone());

                            // Return a reference to the named schema
                            SchemaType::Reference {
                                target: object_type_name,
                            }
                        }
                        OpenApiSchemaType::Array => {
                            // Array of arrays - recursively analyze
                            self.analyze_array_schema(
                                items_schema,
                                parent_schema_name,
                                dependencies,
                            )?
                        }
                        _ => SchemaType::Primitive {
                            rust_type: "serde_json::Value".to_string(),
                        },
                    }
                }
                Schema::OneOf { .. } | Schema::AnyOf { .. } => {
                    // Union types in arrays - analyze recursively
                    let analyzed = self.analyze_schema_value(items_schema, "ArrayItem")?;

                    // If we got a discriminated union or union, we need to create a separate schema for it
                    match &analyzed.schema_type {
                        SchemaType::DiscriminatedUnion { .. } | SchemaType::Union { .. } => {
                            // Generate a unique name for the union schema based on the parent context
                            // Use the parent context directly to maintain consistent naming
                            let union_name = format!("{parent_schema_name}ItemUnion");

                            // Create a new analyzed schema with the correct name
                            let mut union_schema = analyzed;
                            union_schema.name = union_name.clone();

                            // Add the union as a separate schema
                            self.resolved_cache.insert(union_name.clone(), union_schema);

                            // Add dependency
                            dependencies.insert(union_name.clone());

                            // Return a reference to the union schema
                            SchemaType::Reference { target: union_name }
                        }
                        _ => analyzed.schema_type,
                    }
                }
                Schema::Untyped { .. } => {
                    // Try to infer the type
                    if let Some(inferred) = items_schema.inferred_type() {
                        match inferred {
                            OpenApiSchemaType::Object => {
                                // Inline object in array - create a named schema for it
                                let object_type_name = format!("{parent_schema_name}Item");

                                // Analyze the object schema
                                let object_type =
                                    self.analyze_object_schema(items_schema, dependencies)?;

                                // Create an analyzed schema for the inline object
                                let inline_schema = AnalyzedSchema {
                                    name: object_type_name.clone(),
                                    original: serde_json::to_value(items_schema)
                                        .unwrap_or(Value::Null),
                                    schema_type: object_type,
                                    dependencies: dependencies.clone(),
                                    nullable: false,
                                    description: items_schema.details().description.clone(),
                                    default: None,
                                };

                                // Add the inline object as a named schema
                                self.resolved_cache
                                    .insert(object_type_name.clone(), inline_schema);
                                dependencies.insert(object_type_name.clone());

                                // Return a reference to the named schema
                                SchemaType::Reference {
                                    target: object_type_name,
                                }
                            }
                            OpenApiSchemaType::String => SchemaType::Primitive {
                                rust_type: "String".to_string(),
                            },
                            OpenApiSchemaType::Integer | OpenApiSchemaType::Number => {
                                let details = items_schema.details();
                                let rust_type = self.get_number_rust_type(inferred, details);
                                SchemaType::Primitive { rust_type }
                            }
                            OpenApiSchemaType::Boolean => SchemaType::Primitive {
                                rust_type: "bool".to_string(),
                            },
                            _ => SchemaType::Primitive {
                                rust_type: "serde_json::Value".to_string(),
                            },
                        }
                    } else {
                        SchemaType::Primitive {
                            rust_type: "serde_json::Value".to_string(),
                        }
                    }
                }
                _ => SchemaType::Primitive {
                    rust_type: "serde_json::Value".to_string(),
                },
            };

            Ok(SchemaType::Array {
                item_type: Box::new(item_type),
            })
        } else {
            // No items specified, fall back to generic array
            Ok(SchemaType::Primitive {
                rust_type: "Vec<serde_json::Value>".to_string(),
            })
        }
    }

    fn get_number_rust_type(
        &self,
        schema_type: OpenApiSchemaType,
        details: &crate::openapi::SchemaDetails,
    ) -> String {
        match schema_type {
            OpenApiSchemaType::Integer => {
                // Check format field for integer types
                match details.format.as_deref() {
                    Some("int32") => "i32".to_string(),
                    Some("int64") => "i64".to_string(),
                    _ => "i64".to_string(), // Default for integer
                }
            }
            OpenApiSchemaType::Number => {
                // Check format field for number types
                match details.format.as_deref() {
                    Some("float") => "f32".to_string(),
                    Some("double") => "f64".to_string(),
                    _ => "f64".to_string(), // Default for number
                }
            }
            _ => "serde_json::Value".to_string(), // Fallback
        }
    }

    fn analyze_anyof_union(
        &mut self,
        any_of_schemas: &[Schema],
        discriminator: Option<&Discriminator>,
        dependencies: &mut HashSet<String>,
        context_name: &str,
    ) -> Result<SchemaType> {
        // Analyze the semantics of this anyOf

        // Pattern 1: Nullable type [Type, null]
        if any_of_schemas.len() == 2 {
            let null_count = any_of_schemas
                .iter()
                .filter(|s| matches!(s.schema_type(), Some(OpenApiSchemaType::Null)))
                .count();
            if null_count == 1 {
                // This is a nullable pattern - find the non-null type
                for schema in any_of_schemas {
                    if !matches!(schema.schema_type(), Some(OpenApiSchemaType::Null)) {
                        // For nullable pattern, return the non-null type directly
                        // The nullable information is handled at the property level
                        return self
                            .analyze_schema_value(schema, context_name)
                            .map(|a| a.schema_type);
                    }
                }
            }
        }

        // Pattern 2: Multiple complex types or mixed primitive/complex = flexible union
        let has_refs = any_of_schemas.iter().any(|s| s.is_reference());
        let has_objects = any_of_schemas.iter().any(|s| {
            matches!(s.schema_type(), Some(OpenApiSchemaType::Object))
                || s.inferred_type() == Some(OpenApiSchemaType::Object)
        });
        let has_arrays = any_of_schemas
            .iter()
            .any(|s| matches!(s.schema_type(), Some(OpenApiSchemaType::Array)));

        // Handle mixed primitive and complex types (like string + array of objects)
        // Skip this pattern if all schemas are strings or const values (handle in pattern 3)
        let all_string_like = any_of_schemas.iter().all(|s| {
            matches!(s.schema_type(), Some(OpenApiSchemaType::String))
                || s.details().const_value.is_some()
        });

        if (has_refs || has_objects || has_arrays || any_of_schemas.len() > 1) && !all_string_like {
            // Check if this is a discriminated union
            if let Some(disc) = discriminator {
                // This is a discriminated anyOf union, analyze it the same way as oneOf
                return self.analyze_oneof_union(any_of_schemas, Some(disc), None, dependencies);
            }

            // Auto-detect implicit discriminator from const fields across all variants
            if let Some(disc_field) = self.detect_discriminator_field(any_of_schemas) {
                return self.analyze_oneof_union(
                    any_of_schemas,
                    Some(&Discriminator {
                        property_name: disc_field,
                        mapping: None,
                        extra: BTreeMap::new(),
                    }),
                    None,
                    dependencies,
                );
            }

            // Create an untagged union for flexible matching
            let mut variants = Vec::new();

            for schema in any_of_schemas {
                if let Some(ref_str) = schema.reference() {
                    if let Some(target) = self.extract_schema_name(ref_str) {
                        dependencies.insert(target.to_string());
                        variants.push(SchemaRef {
                            target: target.to_string(),
                            nullable: false,
                        });
                    }
                } else if matches!(schema.schema_type(), Some(OpenApiSchemaType::Object))
                    || schema.inferred_type() == Some(OpenApiSchemaType::Object)
                {
                    // Generate inline object type for anyOf union
                    let inline_index = variants.len();
                    let inline_type_name = self.generate_inline_type_name(schema, inline_index);

                    // Store inline schema for later analysis and generation
                    self.add_inline_schema(&inline_type_name, schema, dependencies)?;

                    variants.push(SchemaRef {
                        target: inline_type_name,
                        nullable: false,
                    });
                } else if matches!(schema.schema_type(), Some(OpenApiSchemaType::Array)) {
                    // Handle array types in unions by creating a type alias
                    let array_type =
                        self.analyze_array_schema(schema, context_name, dependencies)?;

                    // Create a unique name for this array type in the union
                    let array_type_name = if let Some(items_schema) = &schema.details().items {
                        if let Some(ref_str) = items_schema.reference() {
                            if let Some(item_type_name) = self.extract_schema_name(ref_str) {
                                dependencies.insert(item_type_name.to_string());
                                format!("{item_type_name}Array")
                            } else {
                                self.generate_context_aware_name(
                                    context_name,
                                    "Array",
                                    variants.len(),
                                    Some(schema),
                                )
                            }
                        } else {
                            self.generate_context_aware_name(
                                context_name,
                                "Array",
                                variants.len(),
                                Some(schema),
                            )
                        }
                    } else {
                        self.generate_context_aware_name(
                            context_name,
                            "Array",
                            variants.len(),
                            Some(schema),
                        )
                    };

                    // Store the array as a type alias
                    self.resolved_cache.insert(
                        array_type_name.clone(),
                        AnalyzedSchema {
                            name: array_type_name.clone(),
                            original: serde_json::to_value(schema).unwrap_or(Value::Null),
                            schema_type: array_type,
                            dependencies: HashSet::new(),
                            nullable: false,
                            description: Some("Array variant in union".to_string()),
                            default: None,
                        },
                    );

                    // Add array type as a dependency
                    dependencies.insert(array_type_name.clone());

                    variants.push(SchemaRef {
                        target: array_type_name,
                        nullable: false,
                    });
                } else if let Some(schema_type) = schema.schema_type() {
                    // Handle primitive types by creating type aliases for consistency
                    let inline_index = variants.len();

                    // Generate a better name for primitive types
                    let inline_type_name = match schema_type {
                        OpenApiSchemaType::String => {
                            // For string types, check if we can infer a better name from context
                            // If this is the first variant and it's a string, use a simple name
                            if inline_index == 0 {
                                format!("{context_name}String")
                            } else {
                                format!("{context_name}StringVariant{inline_index}")
                            }
                        }
                        OpenApiSchemaType::Number => {
                            if inline_index == 0 {
                                format!("{context_name}Number")
                            } else {
                                format!("{context_name}NumberVariant{inline_index}")
                            }
                        }
                        OpenApiSchemaType::Integer => {
                            if inline_index == 0 {
                                format!("{context_name}Integer")
                            } else {
                                format!("{context_name}IntegerVariant{inline_index}")
                            }
                        }
                        OpenApiSchemaType::Boolean => {
                            if inline_index == 0 {
                                format!("{context_name}Boolean")
                            } else {
                                format!("{context_name}BooleanVariant{inline_index}")
                            }
                        }
                        _ => format!("{context_name}Variant{inline_index}"),
                    };

                    let rust_type =
                        self.openapi_type_to_rust_type(schema_type.clone(), schema.details());

                    // Store as a type alias
                    self.resolved_cache.insert(
                        inline_type_name.clone(),
                        AnalyzedSchema {
                            name: inline_type_name.clone(),
                            original: serde_json::to_value(schema).unwrap_or(Value::Null),
                            schema_type: SchemaType::Primitive { rust_type },
                            dependencies: HashSet::new(),
                            nullable: false,
                            description: schema.details().description.clone(),
                            default: None,
                        },
                    );

                    // Add inline type as a dependency
                    dependencies.insert(inline_type_name.clone());

                    variants.push(SchemaRef {
                        target: inline_type_name,
                        nullable: false,
                    });
                }
            }

            if !variants.is_empty() {
                return Ok(SchemaType::Union { variants });
            }
        }

        // Pattern 3: String enum pattern (mix of "type": "string" and const values)
        let all_strings = any_of_schemas.iter().all(|schema| {
            matches!(schema.schema_type(), Some(OpenApiSchemaType::String))
                || schema.details().const_value.is_some()
        });

        if all_strings {
            // Collect all constant values as enum variants
            let mut enum_values = Vec::new();
            let mut has_open_string = false;

            for schema in any_of_schemas {
                if let Some(const_val) = &schema.details().const_value {
                    if let Some(const_str) = const_val.as_str() {
                        enum_values.push(const_str.to_string());
                    }
                } else if matches!(schema.schema_type(), Some(OpenApiSchemaType::String)) {
                    has_open_string = true;
                }
            }

            if !enum_values.is_empty() {
                if has_open_string {
                    // Has both constants and open string - create an extensible enum
                    // This generates an enum with known variants plus a Custom(String) variant
                    return Ok(SchemaType::ExtensibleEnum {
                        known_values: enum_values,
                    });
                } else {
                    // All constants - create string enum
                    return Ok(SchemaType::StringEnum {
                        values: enum_values,
                    });
                }
            }
        }

        // Pattern 4: Mixed primitives = fall back to serde_json::Value
        Ok(SchemaType::Primitive {
            rust_type: "serde_json::Value".to_string(),
        })
    }

    /// Find the schema with $recursiveAnchor: true for resolving $recursiveRef: "#"
    fn find_recursive_anchor_schema(&self) -> Option<String> {
        // Search through all schemas to find one with $recursiveAnchor: true
        for (schema_name, schema) in &self.schemas {
            let details = schema.details();
            if details.recursive_anchor == Some(true) {
                return Some(schema_name.clone());
            }
        }

        // If no schema has $recursiveAnchor: true, this might be an older spec
        // In that case, $recursiveRef: "#" typically refers to the root schema
        // For now, return None to indicate we couldn't resolve it
        None
    }

    /// Detect if a schema should use serde_json::Value for dynamic JSON
    /// Based on structural patterns identified in real-world APIs
    fn should_use_dynamic_json(&self, schema: &Schema) -> bool {
        // Pattern 1: anyOf with [object, null] where object has no properties
        if let Schema::AnyOf { any_of, .. } = schema {
            if any_of.len() == 2 {
                let has_null = any_of
                    .iter()
                    .any(|s| matches!(s.schema_type(), Some(OpenApiSchemaType::Null)));
                let has_empty_object = any_of.iter().any(|s| self.is_dynamic_object_pattern(s));

                if has_null && has_empty_object {
                    return true;
                }
            }
        }

        // Pattern 2: Direct empty object pattern
        self.is_dynamic_object_pattern(schema)
    }

    /// Check if a schema represents a dynamic object pattern
    fn is_dynamic_object_pattern(&self, schema: &Schema) -> bool {
        // Must be object type or untyped with object inference
        let is_object = match schema.schema_type() {
            Some(OpenApiSchemaType::Object) => true,
            None => schema.inferred_type() == Some(OpenApiSchemaType::Object),
            _ => false,
        };

        if !is_object {
            return false;
        }

        let details = schema.details();

        // If it has explicit additionalProperties, it should remain as a typed object
        // that will be generated as BTreeMap<String, serde_json::Value> or similar
        if self.has_explicit_additional_properties(schema) {
            return false;
        }

        // Pattern 1: Object with no properties at all (and no additionalProperties)
        let no_properties = details
            .properties
            .as_ref()
            .map(|props| props.is_empty())
            .unwrap_or(true);

        if no_properties {
            // Check for constraints that would make this a structured type
            let has_structural_constraints =
                // Has required fields (other than just 'type')
                details.required.as_ref()
                    .map(|req| req.iter().any(|r| r != "type"))
                    .unwrap_or(false)
                // Has pattern-based property definitions    
                || details.extra.contains_key("patternProperties")
                // Has property name schema
                || details.extra.contains_key("propertyNames")
                // Has min/max property constraints
                || details.extra.contains_key("minProperties")
                || details.extra.contains_key("maxProperties")
                // Has specific property dependencies
                || details.extra.contains_key("dependencies")
                // Has conditional schemas
                || details.extra.contains_key("if")
                || details.extra.contains_key("then")
                || details.extra.contains_key("else");

            return !has_structural_constraints;
        }

        false
    }

    /// Check if this is an object that explicitly allows arbitrary additional properties
    fn has_explicit_additional_properties(&self, schema: &Schema) -> bool {
        let details = schema.details();

        // Check if additionalProperties is explicitly set to true or a schema
        matches!(
            &details.additional_properties,
            Some(crate::openapi::AdditionalProperties::Boolean(true))
                | Some(crate::openapi::AdditionalProperties::Schema(_))
        )
    }

    /// Analyze OpenAPI operations to extract request/response schemas
    fn analyze_operations(&mut self, analysis: &mut SchemaAnalysis) -> Result<()> {
        let spec: crate::openapi::OpenApiSpec = serde_json::from_value(self.openapi_spec.clone())
            .map_err(GeneratorError::ParseError)?;

        if let Some(paths) = &spec.paths {
            for (path, path_item) in paths {
                for (method, operation) in path_item.operations() {
                    // Generate operation ID if missing
                    let operation_id = operation
                        .operation_id
                        .clone()
                        .unwrap_or_else(|| Self::generate_operation_id(method, path));

                    let op_info = self.analyze_single_operation(
                        &operation_id,
                        method,
                        path,
                        operation,
                        analysis,
                    )?;
                    analysis.operations.insert(operation_id, op_info);
                }
            }
        }
        Ok(())
    }

    /// Generate an operation ID from method and path when not provided
    /// Converts paths like "/v0/servers/{serverId}" + "get" to "getV0ServersServerId"
    fn generate_operation_id(method: &str, path: &str) -> String {
        // Start with the HTTP method in lowercase
        let mut operation_id = method.to_lowercase();

        // Process the path: remove leading slash, split by /, convert to camelCase
        let path_parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();

        for part in path_parts {
            if part.is_empty() {
                continue;
            }

            // Handle path parameters: {serverId} -> ServerId
            let cleaned_part = if part.starts_with('{') && part.ends_with('}') {
                &part[1..part.len() - 1]
            } else {
                part
            };

            // Convert to PascalCase and append
            let pascal_case_part = cleaned_part
                .split(&['-', '_'][..])
                .map(|s| {
                    let mut chars = s.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    }
                })
                .collect::<String>();

            operation_id.push_str(&pascal_case_part);
        }

        operation_id
    }

    /// Analyze a single OpenAPI operation
    fn analyze_single_operation(
        &mut self,
        operation_id: &str,
        method: &str,
        path: &str,
        operation: &crate::openapi::Operation,
        _analysis: &mut SchemaAnalysis,
    ) -> Result<OperationInfo> {
        let mut op_info = OperationInfo {
            operation_id: operation_id.to_string(),
            method: method.to_uppercase(),
            path: path.to_string(),
            request_body: None,
            response_schemas: BTreeMap::new(),
            parameters: Vec::new(),
            supports_streaming: false, // Will be determined by StreamingConfig, not spec
            stream_parameter: None,    // Will be determined by StreamingConfig, not spec
        };

        // Extract request body schema with content-type awareness
        if let Some(request_body) = &operation.request_body
            && let Some((content_type, maybe_schema)) = request_body.best_content()
        {
            op_info.request_body = match content_type {
                "application/json" => maybe_schema
                    .map(|s| {
                        self.resolve_or_inline_schema(s, operation_id, "Request")
                            .map(|name| RequestBodyContent::Json { schema_name: name })
                    })
                    .transpose()?,
                "application/x-www-form-urlencoded" => maybe_schema
                    .map(|s| {
                        self.resolve_or_inline_schema(s, operation_id, "Request")
                            .map(|name| RequestBodyContent::FormUrlEncoded { schema_name: name })
                    })
                    .transpose()?,
                "multipart/form-data" => Some(RequestBodyContent::Multipart),
                "application/octet-stream" => Some(RequestBodyContent::OctetStream),
                "text/plain" => Some(RequestBodyContent::TextPlain),
                _ => None,
            };
        }

        // Extract response schemas
        if let Some(responses) = &operation.responses {
            for (status_code, response) in responses {
                if let Some(schema) = response.json_schema() {
                    if let Some(schema_ref) = schema.reference() {
                        // Named schema reference
                        if let Some(schema_name) = self.extract_schema_name(schema_ref) {
                            op_info
                                .response_schemas
                                .insert(status_code.clone(), schema_name.to_string());
                        }
                    } else {
                        // Inline schema - generate a synthetic type name and analyze it
                        let synthetic_name =
                            self.generate_inline_response_type_name(operation_id, status_code);

                        // Use the existing inline schema infrastructure
                        let mut deps = HashSet::new();
                        self.add_inline_schema(&synthetic_name, schema, &mut deps)?;

                        op_info
                            .response_schemas
                            .insert(status_code.clone(), synthetic_name);
                    }
                }
            }
        }

        // Extract parameters
        if let Some(parameters) = &operation.parameters {
            for param in parameters {
                if let Some(param_info) = self.analyze_parameter(param)? {
                    op_info.parameters.push(param_info);
                }
            }
        }

        Ok(op_info)
    }

    /// Generate a type name for an inline response schema
    fn generate_inline_response_type_name(&self, operation_id: &str, _status_code: &str) -> String {
        use heck::ToPascalCase;
        // Convert operation_id to PascalCase and append Response
        // e.g., "app.skills" -> "AppSkillsResponse"
        // e.g., "getUser" + "200" -> "GetUserResponse"
        let base_name = operation_id.replace('.', "_").to_pascal_case();
        format!("{}Response", base_name)
    }

    /// Generate a type name for an inline request body schema
    fn generate_inline_request_type_name(&self, operation_id: &str) -> String {
        use heck::ToPascalCase;
        // Convert operation_id to PascalCase and append Request
        // e.g., "session.prompt" -> "SessionPromptRequest"
        // e.g., "pty.create" -> "PtyCreateRequest"
        let base_name = operation_id.replace('.', "_").to_pascal_case();
        format!("{}Request", base_name)
    }

    /// Resolve a schema reference to a name, or inline it with a synthetic name.
    /// `suffix` controls the generated name (e.g. "Request" or "Response").
    fn resolve_or_inline_schema(
        &mut self,
        schema: &crate::openapi::Schema,
        operation_id: &str,
        suffix: &str,
    ) -> Result<String> {
        if let Some(schema_ref) = schema.reference()
            && let Some(schema_name) = self.extract_schema_name(schema_ref)
        {
            return Ok(schema_name.to_string());
        }
        // Inline schema - generate a synthetic type name and analyze it
        let synthetic_name = if suffix == "Request" {
            self.generate_inline_request_type_name(operation_id)
        } else {
            self.generate_inline_response_type_name(operation_id, "")
        };
        let mut deps = HashSet::new();
        self.add_inline_schema(&synthetic_name, schema, &mut deps)?;
        Ok(synthetic_name)
    }

    /// Analyze a parameter
    fn analyze_parameter(
        &self,
        param: &crate::openapi::Parameter,
    ) -> Result<Option<ParameterInfo>> {
        let name = param.name.as_deref().unwrap_or("");
        let location = param.location.as_deref().unwrap_or("");
        let required = param.required.unwrap_or(false);

        let mut rust_type = "String".to_string();
        let mut schema_ref = None;

        if let Some(schema) = &param.schema {
            if let Some(ref_str) = schema.reference() {
                schema_ref = self.extract_schema_name(ref_str).map(|s| s.to_string());
            } else if let Some(schema_type) = schema.schema_type() {
                rust_type = match schema_type {
                    crate::openapi::SchemaType::Boolean => "bool",
                    crate::openapi::SchemaType::Integer => "i64",
                    crate::openapi::SchemaType::Number => "f64",
                    crate::openapi::SchemaType::String => "String",
                    _ => "String",
                }
                .to_string();
            }
        }

        Ok(Some(ParameterInfo {
            name: name.to_string(),
            location: location.to_string(),
            required,
            schema_ref,
            rust_type,
        }))
    }
}
