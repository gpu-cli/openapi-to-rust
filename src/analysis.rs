use crate::openapi::{Discriminator, OpenApiSpec, Schema, SchemaType as OpenApiSchemaType};
use crate::type_mapping::TypeMapper;
use crate::{GeneratorError, Result};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// Q2.6 — pull `x-enum-varnames` / `x-enum-descriptions` arrays off
/// the schema's original JSON. Both extensions must be string arrays
/// matching the enum-value count; mismatched extensions are dropped
/// with a stderr warning so they can't subtly break codegen.
///
/// Returns `None` when neither extension is present.
fn extract_enum_extensions(
    original: &Value,
    enum_value_count: usize,
    schema_name: &str,
) -> Option<EnumExtensions> {
    let obj = original.as_object()?;

    let read_string_array = |key: &str| -> Option<Vec<String>> {
        let arr = obj.get(key)?.as_array()?;
        let mut out = Vec::with_capacity(arr.len());
        for v in arr {
            out.push(v.as_str()?.to_string());
        }
        Some(out)
    };

    let varnames_raw = read_string_array("x-enum-varnames");
    let descriptions_raw = read_string_array("x-enum-descriptions");

    if varnames_raw.is_none() && descriptions_raw.is_none() {
        return None;
    }

    let validate = |label: &str, vals: Option<Vec<String>>| -> Vec<String> {
        let Some(vals) = vals else {
            return Vec::new();
        };
        if vals.len() == enum_value_count {
            vals
        } else {
            eprintln!(
                "⚠️  {schema_name}: dropping {label} (expected {enum_value_count} entries, got {})",
                vals.len()
            );
            Vec::new()
        }
    };

    let varnames = validate("x-enum-varnames", varnames_raw);
    let descriptions = validate("x-enum-descriptions", descriptions_raw);

    if varnames.is_empty() && descriptions.is_empty() {
        return None;
    }
    Some(EnumExtensions {
        varnames,
        descriptions,
    })
}

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
    /// Complete response contracts by emitted operation ID and response key.
    /// Unlike `OperationInfo::response_schemas`, this retains responses with
    /// no body as well as their selected JSON media type and SSE declaration.
    pub operation_responses: BTreeMap<String, BTreeMap<String, OperationResponse>>,
    /// Source operationId to emitted operation IDs. Duplicate or
    /// Rust-identifier-colliding IDs are renamed during analysis; retaining
    /// this mapping lets selector resolution report ambiguity or renaming.
    pub operation_id_aliases: BTreeMap<String, Vec<String>>,
    /// Optional crates the [`TypeMapper`] was asked to reference
    /// during analysis (e.g. chrono when a `format: date-time` field
    /// became `chrono::DateTime<Utc>`). The generator reads this to
    /// decide which helper modules (e.g. `base64_serde`) to emit. Complete
    /// dependency reporting is collected from retained emitted files so
    /// pruned schemas cannot leak stale requirements.
    ///
    /// [`TypeMapper`]: crate::type_mapping::TypeMapper
    pub used_type_features: crate::type_mapping::UsedFeatures,
    /// Q2.6: per-schema vendor enum extensions
    /// (`x-enum-varnames` / `x-enum-descriptions`). Populated during
    /// analysis when a StringEnum / ExtensibleEnum schema declares
    /// either extension; the generator uses these to override the
    /// default heuristic variant names and emit per-variant doc
    /// comments. Indexed by analyzed-schema name. Side-channel so we
    /// don't have to touch every StringEnum constructor.
    pub enum_extensions: BTreeMap<String, EnumExtensions>,
    /// Raw, unpruned schema material used to build offline server validators.
    /// This is deliberately independent of `schemas`, which model pruning may
    /// mutate before server artifacts are emitted.
    pub validation_context: ValidationContext,
}

impl SchemaType {
    /// Whether the generator can render this type directly in a field or
    /// element position.
    ///
    /// The other variants name something that must be generated as its own
    /// item — a struct, an enum, a union — so a field can only hold them by
    /// reference. Analysis hoists those and leaves a
    /// [`SchemaType::Reference`]; anything that reaches the generator
    /// un-hoisted is rendered as `serde_json::Value`, losing the type the
    /// schema had. [`UntypedReason::inline_drop`] names those cases so the
    /// census can count them.
    pub fn renders_inline(&self) -> bool {
        match self {
            Self::Primitive { .. }
            | Self::Reference { .. }
            | Self::Array { .. }
            | Self::Tuple { .. }
            | Self::Nullable { .. }
            | Self::Untyped { .. } => true,
            Self::Object { .. }
            | Self::StringEnum { .. }
            | Self::ExtensibleEnum { .. }
            | Self::DiscriminatedUnion { .. }
            | Self::Union { .. }
            | Self::Composition { .. } => false,
        }
    }
}

impl UntypedReason {
    /// The reason a non-inline-renderable type reaching a field position gets
    /// dropped to `serde_json::Value`.
    pub fn inline_drop(schema_type: &SchemaType) -> Option<Self> {
        match schema_type {
            SchemaType::Composition { .. } => Some(Self::InlineCompositionDropped),
            SchemaType::Union { .. } | SchemaType::DiscriminatedUnion { .. } => {
                Some(Self::InlineUnionDropped)
            }
            SchemaType::Object { .. } => Some(Self::InlineObjectDropped),
            SchemaType::StringEnum { .. } | SchemaType::ExtensibleEnum { .. } => {
                Some(Self::InlineEnumDropped)
            }
            _ => None,
        }
    }
}

/// The schemas a generated type refers to.
///
/// Synthesized types — a hoisted property type, a named union — need these to
/// be accurate, not merely non-empty: the dependency graph is what
/// `detect_recursive_schemas` reads, and a cycle that runs through a
/// synthesized type is invisible without them. Stripe's
/// `Quote → QuotesResourceFromQuote → QuotesResourceFromQuoteQuote → Quote`
/// compiled to an infinitely-sized enum until the middle link declared where it
/// pointed.
fn schema_type_dependencies(schema_type: &SchemaType) -> HashSet<String> {
    let mut targets = HashSet::new();
    collect_type_dependencies(schema_type, &mut targets, 0);
    targets
}

fn collect_type_dependencies(
    schema_type: &SchemaType,
    targets: &mut HashSet<String>,
    depth: usize,
) {
    if depth > UNTYPED_WALK_DEPTH {
        return;
    }
    match schema_type {
        SchemaType::Reference { target } => {
            targets.insert(target.clone());
        }
        SchemaType::Array { item_type } => collect_type_dependencies(item_type, targets, depth + 1),
        SchemaType::Nullable { inner_type } => {
            collect_type_dependencies(inner_type, targets, depth + 1)
        }
        SchemaType::Tuple { element_types } => {
            for element_type in element_types {
                collect_type_dependencies(element_type, targets, depth + 1);
            }
        }
        SchemaType::Object {
            properties,
            additional_properties,
            ..
        } => {
            for property in properties.values() {
                collect_type_dependencies(&property.schema_type, targets, depth + 1);
            }
            if let ObjectAdditionalProperties::Typed { value_type } = additional_properties {
                collect_type_dependencies(value_type, targets, depth + 1);
            }
        }
        SchemaType::Union { variants, .. } | SchemaType::Composition { schemas: variants } => {
            for variant in variants {
                targets.insert(variant.target.clone());
            }
        }
        SchemaType::DiscriminatedUnion { variants, .. } => {
            for variant in variants {
                targets.insert(variant.type_name.clone());
            }
        }
        SchemaType::Primitive { .. }
        | SchemaType::StringEnum { .. }
        | SchemaType::ExtensibleEnum { .. }
        | SchemaType::Untyped { .. } => {}
    }
}

/// Convert any `serde_json::Value` still carried as a stringly-typed
/// `Primitive` into [`SchemaType::Untyped`].
///
/// Several fallbacks build their type from a [`TypeMapper`] result rather than
/// through the analyzer's helpers, so this runs over the finished IR as a net.
/// Anything it catches is reported as [`UntypedReason::Unclassified`] — a
/// visible gap in the taxonomy rather than a silently missing count.
///
/// [`TypeMapper`]: crate::type_mapping::TypeMapper
fn normalize_untyped(schema_type: &mut SchemaType, depth: usize) {
    if depth > UNTYPED_WALK_DEPTH {
        return;
    }
    match schema_type {
        SchemaType::Primitive { rust_type, .. } => {
            let shape = match rust_type.as_str() {
                "serde_json::Value" => Some(UntypedShape::Value),
                "Vec<serde_json::Value>" => Some(UntypedShape::ValueArray),
                _ => None,
            };
            if let Some(shape) = shape {
                *schema_type = SchemaType::Untyped {
                    shape,
                    reason: UntypedReason::Unclassified,
                };
            }
        }
        SchemaType::Object {
            properties,
            additional_properties,
            ..
        } => {
            for property in properties.values_mut() {
                normalize_untyped(&mut property.schema_type, depth + 1);
            }
            if let ObjectAdditionalProperties::Typed { value_type } = additional_properties {
                normalize_untyped(value_type, depth + 1);
            }
        }
        SchemaType::Array { item_type } => normalize_untyped(item_type, depth + 1),
        SchemaType::Nullable { inner_type } => normalize_untyped(inner_type, depth + 1),
        SchemaType::Tuple { element_types } => {
            for element_type in element_types {
                normalize_untyped(element_type, depth + 1);
            }
        }
        SchemaType::Untyped { .. }
        | SchemaType::StringEnum { .. }
        | SchemaType::ExtensibleEnum { .. }
        | SchemaType::DiscriminatedUnion { .. }
        | SchemaType::Union { .. }
        | SchemaType::Composition { .. }
        | SchemaType::Reference { .. } => {}
    }
}

impl SchemaAnalysis {
    /// Every generated field that carries `serde_json::Value`, with the reason.
    ///
    /// Derived from the analyzed types rather than recorded as analysis runs,
    /// so the count tracks generated output: a schema referenced by fifty
    /// properties contributes fifty findings, and a pruned one contributes
    /// none.
    pub fn untyped_fields(&self) -> Vec<UntypedFinding> {
        let mut findings = Vec::new();
        for (name, schema) in &self.schemas {
            collect_untyped(&schema.schema_type, name, &mut findings, 0);
        }
        findings.sort();
        findings
    }
}

/// Depth limit for the census walk. Generated types bottom out well before
/// this; the bound only stops a cycle that slipped through analysis from
/// hanging a diagnostic.
const UNTYPED_WALK_DEPTH: usize = 32;

fn collect_untyped(
    schema_type: &SchemaType,
    context: &str,
    findings: &mut Vec<UntypedFinding>,
    depth: usize,
) {
    if depth > UNTYPED_WALK_DEPTH {
        return;
    }
    match schema_type {
        SchemaType::Untyped { shape, reason } => findings.push(UntypedFinding {
            context: context.to_string(),
            shape: *shape,
            reason: *reason,
        }),
        SchemaType::Object {
            properties,
            additional_properties,
            ..
        } => {
            for (property_name, property) in properties {
                let property_context = format!("{context}.{property_name}");
                // A type the generator cannot render inline is dropped whole:
                // count it here rather than descending into a type that will
                // never reach the output.
                if let Some(reason) = UntypedReason::inline_drop(&property.schema_type) {
                    findings.push(UntypedFinding {
                        context: property_context,
                        shape: UntypedShape::Value,
                        reason,
                    });
                    continue;
                }
                collect_untyped(
                    &property.schema_type,
                    &property_context,
                    findings,
                    depth + 1,
                );
            }
            match additional_properties {
                ObjectAdditionalProperties::Untyped => findings.push(UntypedFinding {
                    context: format!("{context}.<additionalProperties>"),
                    shape: UntypedShape::ValueMap,
                    reason: UntypedReason::UntypedAdditionalProperties,
                }),
                ObjectAdditionalProperties::Typed { value_type } => collect_untyped(
                    value_type,
                    &format!("{context}.<additionalProperties>"),
                    findings,
                    depth + 1,
                ),
                ObjectAdditionalProperties::Forbidden => {}
            }
        }
        SchemaType::Array { item_type } => {
            let element_context = format!("{context}[]");
            if let Some(reason) = UntypedReason::inline_drop(item_type) {
                findings.push(UntypedFinding {
                    context: element_context,
                    shape: UntypedShape::Value,
                    reason,
                });
            } else {
                collect_untyped(item_type, &element_context, findings, depth + 1);
            }
        }
        SchemaType::Nullable { inner_type } => {
            collect_untyped(inner_type, context, findings, depth + 1)
        }
        SchemaType::Tuple { element_types } => {
            for (index, element_type) in element_types.iter().enumerate() {
                collect_untyped(
                    element_type,
                    &format!("{context}[{index}]"),
                    findings,
                    depth + 1,
                );
            }
        }
        // A union branch that mapped to an untyped Rust type is carried as a
        // variant target string, so it is recognized by name here.
        SchemaType::Union { variants, .. } | SchemaType::Composition { schemas: variants } => {
            for (index, variant) in variants.iter().enumerate() {
                if let Some(shape) = untyped_shape_of(&variant.target) {
                    findings.push(UntypedFinding {
                        context: format!("{context}|{index}"),
                        shape,
                        reason: UntypedReason::UntypedUnionBranch,
                    });
                }
            }
        }
        SchemaType::Primitive { .. }
        | SchemaType::StringEnum { .. }
        | SchemaType::ExtensibleEnum { .. }
        | SchemaType::DiscriminatedUnion { .. }
        | SchemaType::Reference { .. } => {}
    }
}

/// The untyped shape a generated Rust type name denotes, if any.
fn untyped_shape_of(rust_type: &str) -> Option<UntypedShape> {
    match rust_type {
        "serde_json::Value" => Some(UntypedShape::Value),
        "Vec<serde_json::Value>" => Some(UntypedShape::ValueArray),
        _ => None,
    }
}

/// One generated field (or type) that carries `serde_json::Value` instead of a
/// generated Rust type.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct UntypedFinding {
    /// Where it surfaced, as far as analysis knows: usually
    /// `Schema.property`, or a synthesized operation type.
    pub context: String,
    /// The shape the generator will emit.
    pub shape: UntypedShape,
    /// Why the schema produced no better type.
    pub reason: UntypedReason,
}

/// The generated shape carrying the untyped payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UntypedShape {
    /// `serde_json::Value`
    Value,
    /// `Vec<serde_json::Value>`
    ValueArray,
    /// `BTreeMap<String, serde_json::Value>`
    ValueMap,
}

/// Why a schema produced an untyped value.
///
/// The split that matters is [`UntypedReason::verdict`]: a schema that says
/// "any JSON" has no better Rust type and is generated correctly, while a
/// schema that carried type information the generator dropped is a defect with
/// a fix. Counting the two together would make the corpus look worse than it is
/// and hide which cases are worth work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UntypedReason {
    /// `{}`, `true`, or a schema with no constraining keyword at all: the spec
    /// declares any JSON value.
    AnySchema,
    /// `type: object` with no `properties` and no typed `additionalProperties`:
    /// an object of unknown shape.
    OpaqueObject,
    /// `additionalProperties: true` or absent, so map values are unconstrained.
    UntypedAdditionalProperties,
    /// `type: array` with no `items` at all.
    ArrayWithoutItems,
    /// Positional items that permit extra elements of any type (issue #62,
    /// tier 3), so neither a tuple nor a `Vec<T>` is sound.
    OpenPositionalItems,
    /// A union (`oneOf`/`anyOf`) whose branches did not reduce to one
    /// generated Rust type.
    UnrepresentableUnion,
    /// An `allOf` composition that could not be merged into a struct.
    UnrepresentableComposition,
    /// The schema declared a type keyword the generator has no mapping for.
    UnsupportedTypeKeyword,
    /// A `$ref` that analysis could not resolve to a generated schema.
    UnresolvedReference,
    /// An `allOf` composition sitting in a field position. Analysis did not
    /// merge or hoist it, and a field cannot hold one, so the generator emits
    /// `serde_json::Value` — dropping a type the schema fully described. A
    /// single-branch `allOf` around a scalar is the common shape.
    InlineCompositionDropped,
    /// A union in a field position that was never hoisted to a named enum.
    InlineUnionDropped,
    /// An inline object in a field position that was never hoisted to a struct.
    InlineObjectDropped,
    /// An inline enum in a field position that was never hoisted.
    InlineEnumDropped,
    /// A `oneOf`/`anyOf` branch that mapped to an untyped value, so the
    /// generated union carries a `serde_json::Value` variant. Whether that is
    /// faithful depends on the branch, which the analyzed type no longer says.
    UntypedUnionBranch,
    /// A `false` schema: nothing validates against it, so there is no value to
    /// give a type. Legal anywhere a schema is, and written to forbid a
    /// property or close a tuple.
    NeverMatches,
    /// Reached a fallback that has not been classified yet. Every one of these
    /// is a gap in this taxonomy, not in the generator.
    Unclassified,
}

impl UntypedReason {
    /// Whether the untyped output is the honest reading of the schema, or a
    /// case where the generator can do better.
    pub fn verdict(self) -> UntypedVerdict {
        match self {
            // The spec genuinely declares an unconstrained value.
            Self::AnySchema | Self::OpaqueObject | Self::UntypedAdditionalProperties => {
                UntypedVerdict::Faithful
            }
            // Nothing validates against `false`, so nothing is being lost.
            Self::NeverMatches => UntypedVerdict::Faithful,
            // An array with no `items` says nothing about elements, and open
            // positional items permit extras of any type: both are the spec's
            // choice, not a dropped constraint.
            Self::ArrayWithoutItems | Self::OpenPositionalItems => UntypedVerdict::Faithful,
            // The schema described a type that the generator then dropped
            // because nothing hoisted it out of the field position.
            Self::InlineCompositionDropped
            | Self::InlineUnionDropped
            | Self::InlineObjectDropped
            | Self::InlineEnumDropped => UntypedVerdict::Recoverable,
            // These carried type information that did not survive analysis.
            Self::UnrepresentableUnion
            | Self::UnrepresentableComposition
            | Self::UnsupportedTypeKeyword
            | Self::UnresolvedReference => UntypedVerdict::Recoverable,
            Self::UntypedUnionBranch | Self::Unclassified => UntypedVerdict::Unknown,
        }
    }
}

/// Whether an untyped output is worth working on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UntypedVerdict {
    /// The schema declares an unconstrained value; `serde_json::Value` is correct.
    Faithful,
    /// The schema carried type information the generator dropped.
    Recoverable,
    /// Not yet classified.
    Unknown,
}

/// Server-relevant semantics of one OpenAPI Response Object.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct OperationResponse {
    /// Generated Rust body type for the preferred JSON-compatible content.
    pub schema_name: Option<String>,
    /// Exact declared JSON-compatible media type selected for `schema_name`.
    pub media_type: Option<String>,
    /// Preferred buffered response representation for this status. JSON keeps
    /// its generated schema name; text and binary bodies are represented
    /// directly by the generated client/server runtime types.
    pub body: Option<OperationResponseBody>,
    /// Whether this response also declares `text/event-stream` content.
    pub supports_streaming: bool,
    /// Whether the Response Object declared at least one content entry.
    pub has_content: bool,
    /// Declared response media types the server generator cannot emit.
    pub unsupported_media_types: Vec<String>,
}

/// Buffered response representation selected from one OpenAPI Response Object.
/// SSE remains orthogonal on [`OperationResponse::supports_streaming`] because
/// a response may advertise both a buffered JSON representation and an event
/// stream.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OperationResponseBody {
    Json {
        schema_name: String,
        media_type: String,
    },
    Text {
        media_type: String,
    },
    Binary {
        media_type: String,
        wildcard: bool,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ValidationContext {
    pub openapi_version: String,
    pub json_schema_dialect: Option<String>,
    pub component_schemas: BTreeMap<String, Value>,
}

/// Q2.6 — vendor extensions describing a string enum's variant
/// names and per-variant descriptions. Length must match the
/// schema's `enum` array; mismatched extensions are dropped at
/// analysis time with a warning.
#[derive(Debug, Clone, Default)]
pub struct EnumExtensions {
    /// `x-enum-varnames`: Rust-friendly variant identifiers per
    /// enum value, in the same order as the spec's `enum` array.
    /// When present and length matches, the generator uses these
    /// instead of its default PascalCase heuristic.
    pub varnames: Vec<String>,
    /// `x-enum-descriptions`: one doc-comment per enum value.
    pub descriptions: Vec<String>,
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
    /// Simple primitive type. `serde_with` carries an optional
    /// `#[serde(with = "<path>")]` codec hint produced by the
    /// TypeMapper for typed scalars (e.g. `format: byte` →
    /// `Vec<u8>` + `base64_serde`); the generator wraps this in a
    /// field-level `with = ...` attribute.
    Primitive {
        rust_type: String,
        serde_with: Option<String>,
    },
    /// Object with properties
    Object {
        properties: BTreeMap<String, PropertyInfo>,
        required: HashSet<String>,
        additional_properties: ObjectAdditionalProperties,
        /// A union the schema declares *alongside* its own properties —
        /// `{properties: {...}, anyOf: [...]}` — meaning "these fields, and
        /// one of these shapes". Held in a `#[serde(flatten)]` field so both
        /// halves survive; `None` for a plain object.
        variant: Option<SchemaRef>,
    },
    /// Discriminated union (`oneOf`/`anyOf` + discriminator).
    DiscriminatedUnion {
        discriminator_field: String,
        variants: Vec<UnionVariant>,
        /// `oneOf` requires a unique structural match; `anyOf` permits a
        /// deterministic first match when the discriminator is absent or its
        /// preferred branch does not fit.
        exclusive: bool,
    },
    /// Simple union. Exclusive unions originate from `oneOf` and require
    /// exactly one branch to preserve the complete input shape; non-exclusive
    /// unions retain `anyOf`/multi-type first-match semantics.
    Union {
        variants: Vec<SchemaRef>,
        exclusive: bool,
    },
    /// Array type
    Array { item_type: Box<SchemaType> },
    /// A nullable value in a container position. Object properties carry
    /// nullability separately because their `Option<T>` also participates in
    /// required-vs-missing serde behavior; array items, tuple positions, and
    /// typed additional-property values need an inline wrapper instead.
    Nullable { inner_type: Box<SchemaType> },
    /// Fixed-arity array — one schema per position, no extras — rendered as a
    /// Rust tuple. Only emitted when the spec proves the length (see
    /// `SchemaDetails::positional_items_are_exact`); an open `prefixItems`
    /// stays an `Array`, because serde would reject the extra elements the
    /// spec allows.
    Tuple { element_types: Vec<SchemaType> },
    /// String enum
    StringEnum { values: Vec<String> },
    /// Extensible enum with known values and custom variant
    ExtensibleEnum { known_values: Vec<String> },
    /// Schema composition (allOf)
    Composition { schemas: Vec<SchemaRef> },
    /// Reference to another schema
    Reference { target: String },
    /// A value the generator could not type, rendered as `serde_json::Value`.
    ///
    /// The reason travels with the type rather than in a side table, so the
    /// census counts what is actually generated: a schema analyzed once but
    /// referenced by fifty properties yields fifty untyped fields, and one that
    /// is pruned yields none.
    Untyped {
        shape: UntypedShape,
        reason: UntypedReason,
    },
}

/// How an Object handles `additionalProperties`. Q2.3 split the
/// pre-existing `bool` into a three-way enum so the generator can
/// emit a typed `BTreeMap<String, T>` when the spec provides a
/// value-type schema instead of degrading to `serde_json::Value`.
#[derive(Debug, Clone)]
pub enum ObjectAdditionalProperties {
    /// No catch-all field is emitted. This is exact for
    /// `additionalProperties: false`; for an omitted keyword it is the
    /// generator's historical closed-model projection and is used only while
    /// no required unknown member forces an open carrier.
    Forbidden,
    /// `additionalProperties: true` — extra keys captured as
    /// `BTreeMap<String, serde_json::Value>`.
    Untyped,
    /// `additionalProperties: <schema>` — extra keys captured as
    /// `BTreeMap<String, T>` where T comes from the schema.
    Typed { value_type: Box<SchemaType> },
}

impl ObjectAdditionalProperties {
    /// True when extra keys are accepted (regardless of typing).
    /// Used by callers that only care whether the field exists.
    pub fn is_open(&self) -> bool {
        !matches!(self, Self::Forbidden)
    }
}

#[derive(Debug, Clone)]
pub struct PropertyInfo {
    pub schema_type: SchemaType,
    pub nullable: bool,
    pub description: Option<String>,
    pub default: Option<serde_json::Value>,
    pub serde_attrs: Vec<String>,
    /// True when this field was synthesized from a `required` name that the
    /// schema did not also declare in `properties`. Keeping that provenance
    /// lets allOf merging prefer a real sibling declaration regardless of
    /// branch order.
    pub synthesized_required: bool,
    /// Q2.4: OpenAPI constraint annotations captured from the
    /// property schema. Surfaced by the generator as `/// Constraint:
    /// …` doc lines and/or `#[validate(...)]` attributes depending on
    /// `[generator.types.constraints] mode`.
    pub constraints: PropertyConstraints,
}

/// Q2.4 — per-property OpenAPI constraint annotations
/// (`minimum`/`maximum`/`minLength`/`maxLength`/`pattern`/etc.).
/// Populated during analysis from `SchemaDetails`; consumed by the
/// generator to emit doc comments and/or `#[validate(...)]` attrs.
#[derive(Debug, Clone, Default)]
pub struct PropertyConstraints {
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub exclusive_minimum: Option<f64>,
    pub exclusive_maximum: Option<f64>,
    pub multiple_of: Option<f64>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    pub pattern: Option<String>,
    pub min_items: Option<u64>,
    pub max_items: Option<u64>,
    pub unique_items: Option<bool>,
}

impl PropertyConstraints {
    pub fn is_empty(&self) -> bool {
        self.minimum.is_none()
            && self.maximum.is_none()
            && self.exclusive_minimum.is_none()
            && self.exclusive_maximum.is_none()
            && self.multiple_of.is_none()
            && self.min_length.is_none()
            && self.max_length.is_none()
            && self.pattern.is_none()
            && self.min_items.is_none()
            && self.max_items.is_none()
            && self.unique_items.is_none()
    }

    /// Capture the constraint-related fields off a `SchemaDetails`.
    /// Exclusive bounds in OpenAPI 3.1 are numeric (`exclusiveMinimum:
    /// 5`); we map the OAS-3.0 boolean flag form by leaving the
    /// exclusive field unset and letting `minimum`/`maximum` carry it.
    pub fn from_schema_details(details: &crate::openapi::SchemaDetails) -> Self {
        use crate::openapi::ExclusiveBound;
        let exclusive_minimum = match &details.exclusive_minimum {
            Some(ExclusiveBound::Number(v)) => Some(*v),
            _ => None,
        };
        let exclusive_maximum = match &details.exclusive_maximum {
            Some(ExclusiveBound::Number(v)) => Some(*v),
            _ => None,
        };
        Self {
            minimum: details
                .minimum
                .as_ref()
                .and_then(serde_json::Number::as_f64),
            maximum: details
                .maximum
                .as_ref()
                .and_then(serde_json::Number::as_f64),
            exclusive_minimum,
            exclusive_maximum,
            multiple_of: details.multiple_of,
            min_length: details.min_length,
            max_length: details.max_length,
            pattern: details.pattern.clone(),
            min_items: details.min_items,
            max_items: details.max_items,
            unique_items: details.unique_items,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UnionVariant {
    pub rust_name: String,
    pub type_name: String,
    /// Canonical discriminator value used when the payload does not already
    /// carry one. This is always the first member of
    /// `discriminator_values`.
    pub discriminator_value: String,
    /// Every wire discriminator value accepted by this branch. JSON Schema
    /// permits a discriminator property to use a multi-value enum, so a
    /// branch is not necessarily identified by exactly one string.
    pub discriminator_values: Vec<String>,
    /// Values for which this branch is the preferred first dispatch target.
    /// Overlapping branch constraints remain in `discriminator_values` so
    /// deserialization can fall back structurally when the preferred branch
    /// does not fit the rest of the payload.
    pub preferred_discriminator_values: Vec<String>,
    /// Whether the branch schema declares the discriminator property at all.
    /// A mapped/tagless branch may legitimately omit it, in which case the
    /// serializer must not invent a schema-name-derived wire field.
    pub discriminator_field_declared: bool,
    /// Whether the branch schema requires the discriminator property.
    /// Missing-tag structural fallback is limited to branches where this is
    /// false.
    pub discriminator_field_required: bool,
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
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OperationInfo {
    /// Operation ID
    pub operation_id: String,
    /// HTTP method (GET, POST, etc.)
    pub method: String,
    /// Path template
    pub path: String,
    /// Short summary from OpenAPI spec
    pub summary: Option<String>,
    /// Longer description from OpenAPI spec
    pub description: Option<String>,
    /// Request body content type and schema (if any)
    pub request_body: Option<RequestBodyContent>,
    /// Whether `requestBody.required` was true. Drives whether the generated
    /// method takes a `Body` argument or `Option<Body>` (T11).
    pub request_body_required: bool,
    /// Response schemas by status code
    pub response_schemas: BTreeMap<String, String>,
    /// Parameters (path, query, header)
    pub parameters: Vec<ParameterInfo>,
    /// Whether this operation supports streaming
    pub supports_streaming: bool,
    /// Stream parameter name if applicable
    pub stream_parameter: Option<String>,
    /// Tags declared on the operation. Empty when the spec sets none.
    /// Used by the server codegen selector grammar (e.g. `tag:Chat`)
    /// and by `openapi-to-rust server list` for grouping.
    pub tags: Vec<String>,
}

/// Content type and schema for a request body
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind")]
pub enum RequestBodyContent {
    Json {
        schema_name: String,
        media_type: String,
        #[serde(skip)]
        validation_schema: Value,
    },
    FormUrlEncoded {
        schema_name: String,
        media_type: String,
        #[serde(skip)]
        validation_schema: Value,
    },
    Multipart {
        schema_name: String,
        media_type: String,
        #[serde(skip)]
        validation_schema: Value,
    },
    OctetStream {
        media_type: String,
    },
    Binary {
        media_type: String,
    },
    TextPlain {
        media_type: String,
    },
    /// A declared request media type without a schema. Client generation
    /// preserves its historical no-body signature, while server generation
    /// rejects the operation because there is no contract to validate.
    SchemaLess {
        media_type: String,
    },
    Unsupported {
        media_types: Vec<String>,
    },
}

impl RequestBodyContent {
    /// Get the schema name if this content type has one
    pub fn schema_name(&self) -> Option<&str> {
        match self {
            Self::Json { schema_name, .. }
            | Self::FormUrlEncoded { schema_name, .. }
            | Self::Multipart { schema_name, .. } => Some(schema_name),
            Self::OctetStream { .. }
            | Self::Binary { .. }
            | Self::TextPlain { .. }
            | Self::SchemaLess { .. }
            | Self::Unsupported { .. } => None,
        }
    }
}

/// Compute the disambiguation-base for a parameter name. Mirrors
/// `ClientGenerator::sanitize_param_name` so analysis-time uniqueness
/// decisions and codegen-time emission agree on the final ident.
fn base_param_ident(name: &str) -> String {
    use heck::ToSnakeCase;
    let suffix = if name.ends_with("<=") {
        "_lte"
    } else if name.ends_with(">=") {
        "_gte"
    } else if name.ends_with('<') {
        "_lt"
    } else if name.ends_with('>') {
        "_gt"
    } else {
        ""
    };
    let stripped = name.trim_end_matches(['<', '>', '=']);
    let mut snake = stripped.to_snake_case();
    if snake.is_empty() {
        snake.push_str("parameter");
    } else if snake.starts_with(|character: char| character.is_ascii_digit()) {
        snake.insert(0, '_');
    }
    snake.push_str(suffix);
    snake
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
    /// Description from OpenAPI spec
    pub description: Option<String>,
    /// String enum values when the parameter's inline schema is a string with
    /// `enum` or `const`. When set, `rust_type` is the synthetic enum type
    /// name (e.g. `GetItemTheConstant`) and the client generator emits an
    /// inline enum so the parameter is constrained to the declared values.
    /// See issue #10 follow-up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<String>>,
    /// `x-enum-varnames` declared on the parameter's inline enum schema, when
    /// present and the same length as `enum_values`. Schema-level enums already
    /// honor this vendor extension through `SchemaAnalysis::enum_extensions`;
    /// parameter enums are inline and have no analyzed-schema name to key on,
    /// so their names ride along here instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enum_varnames: Option<Vec<String>>,
    /// Disambiguated Rust ident assigned by the analyzer at the operation
    /// scope. When two parameters in the same operation sanitize to the same
    /// snake_case name (e.g. `exclude_ids` + `exclude-ids` in vercel,
    /// `StartTime` + `StartTime>` in twilio), the analyzer suffixes
    /// later occurrences with `_2`, `_3`, … so the codegen function
    /// signature and body don't reuse the same binding.
    /// Empty/none = use sanitize from `name`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_ident: Option<String>,
    /// Wire serialization for object/array query parameters, decided from
    /// the parameter's `style`/`explode` and schema shape (T14, GH #27).
    /// `None` = plain single `name=value` pair (scalars, string enums, and
    /// the ordinary scalar `name=value` representation. Unsupported complex
    /// shapes carry an explicit [`QuerySerialization::Unsupported`] reason so
    /// downstream client/server generators cannot silently drift.
    /// For the object modes, `schema_ref` holds the struct type
    /// generated/resolved for the object schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_serialization: Option<QuerySerialization>,
    /// Original parameter schema retained for request validation. This is not
    /// exposed by serialized operation listings.
    #[serde(skip)]
    pub validation_schema: Option<Value>,
}

/// How generated clients serialize and generated servers extract an object-
/// or array-schema query parameter.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum QuerySerialization {
    /// style=form + explode=true object (the OAS 3.x defaults for query):
    /// each property is its own pair — `?color=red&size=big`. The parameter
    /// name never appears in the query string (RFC 6570 form-explosion).
    FormExplodedObject,
    /// AWS query-protocol form explosion for an object containing arrays:
    /// `Parameter.Prop.1=value` or `Parameter.Prop.1.Leaf=value`. Unlike
    /// ordinary RFC 6570 form explosion, AWS service models retain the outer
    /// parameter wire name; client and server generation intentionally mirror
    /// that protocol-specific representation.
    FormExplodedNestedObject {
        properties: Vec<QueryStructProperty>,
    },
    /// style=form + explode=false object: one comma-joined key,value list —
    /// `?filter=color,red,size,big`.
    FormObject,
    /// style=deepObject (explode=true) object: bracketed keys —
    /// `?filter[color]=red`.
    DeepObject,
    /// style=form + explode=true array: repeated pairs — `?tags=a&tags=b`.
    /// Parameter typed `Vec<item_type>`.
    FormExplodedArray { item_type: ArrayItemType },
    /// style=form + explode=false array: one comma-joined pair —
    /// `?tags=a,b,c`. Parameter typed `Vec<item_type>`.
    FormArray { item_type: ArrayItemType },
    /// Header `style=simple, explode=false` array: one physical header value
    /// containing comma-separated scalar items.
    SimpleHeaderArray { item_type: ArrayItemType },
    /// A complex query shape whose wire representation is undefined by
    /// OpenAPI or not implemented symmetrically. Clients retain the explicit
    /// opaque-string escape hatch; server generation rejects it with this
    /// actionable reason instead of emitting an impossible extractor.
    Unsupported { reason: String },
}

/// Item type of a typed array query parameter. The two variants need
/// different handling in codegen: scalars are already Rust type strings
/// (possibly paths like `rust_decimal::Decimal` from `[type_mappings]`),
/// while schema refs are raw *schema names* that must run through
/// `to_rust_type_name` sanitization (cloudflare:
/// `resource-sharing_resource_type`).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum ArrayItemType {
    /// A Rust scalar type string from the TypeMapper (`String`, `i32`, …).
    Scalar(String),
    /// The schema name of a referenced scalar alias or string enum.
    SchemaRef(String),
    /// The schema name of a referenced *flat* structure — every property is
    /// scalar. Serialized AWS query-protocol style as
    /// `param.N.Prop=value` per item (e.g. `Tags.1.Key=k&Tags.1.Value=v`).
    /// Carries the wire property names so client and server emit identical
    /// keys without re-resolving the schema.
    FlatStructRef {
        schema_name: String,
        properties: Vec<QueryStructProperty>,
    },
    /// A referenced structure with scalar properties plus arrays whose items
    /// are scalar or flat structures. This is the deepest unambiguous shape
    /// used by AWS query protocols (`param.N.Prop.M.Leaf=value`).
    NestedStructRef {
        schema_name: String,
        properties: Vec<QueryStructProperty>,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct QueryStructProperty {
    pub wire_name: String,
    pub required: bool,
    pub value_type: QueryStructPropertyType,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum QueryStructPropertyType {
    Scalar(QueryScalarType),
    Array {
        item_type: ArrayItemType,
    },
    Object {
        properties: Vec<QueryStructProperty>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum QueryScalarType {
    String,
    Integer,
    Number,
    Boolean,
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

/// AWS-style specs append query markers to their path templates
/// (`/tags/{resourceArn}#tagKeys`, `/2015-02-01/resource-tags/{ResourceId}#tagKeys`).
/// The fragment is not part of the route — those values are declared as
/// ordinary query parameters on the operation — so strip it before the path
/// reaches route generation. Axum (and every HTTP router) matches on the path
/// component only.
fn normalize_operation_path(path: &str) -> String {
    match path.split_once('#') {
        Some((route, _fragment)) if route.starts_with('/') => route.to_string(),
        _ => path.to_string(),
    }
}

/// See through an `allOf: [$ref, {annotation}]` wrapper around a schema, the
/// same shape `analyze_all_of` treats as a type alias. Returns the sole
/// reference target's schema when every other member is annotation-only;
/// otherwise the schema itself.
fn unwrap_annotation_allof(schema: &crate::openapi::Schema) -> &crate::openapi::Schema {
    let crate::openapi::Schema::AllOf { all_of, .. } = schema else {
        return schema;
    };
    let mut references = all_of.iter().filter(|s| s.reference().is_some());
    let (Some(first), None) = (references.next(), references.next()) else {
        return schema;
    };
    let others_annotation_only = all_of
        .iter()
        .all(|member| member.reference().is_some() || schema_is_annotation_only(member));
    if others_annotation_only {
        first
    } else {
        schema
    }
}

/// Whether a schema contributes annotations but no assertion to an
/// intersection. OpenAPI's `nullable` only modifies an adjacent `type`, so a
/// type-less nullable flag is neutral here. `default` and examples are JSON
/// Schema annotations as well.
fn schema_is_annotation_only(schema: &crate::openapi::Schema) -> bool {
    serde_json::to_value(schema)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .is_some_and(|object| {
            object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "title"
                        | "description"
                        | "deprecated"
                        | "readOnly"
                        | "writeOnly"
                        | "examples"
                        | "example"
                        | "default"
                        | "externalDocs"
                        | "xml"
                        | "$comment"
                        | "nullable"
                ) || key.starts_with("x-")
            })
        })
}

/// Load an extension file and parse it into the JSON representation used by
/// the analyzer. YAML extensions follow the same conversion policy as YAML
/// OpenAPI documents; every other extension is parsed as JSON.
fn load_extension_file(path: &Path) -> Result<Value> {
    let content = std::fs::read_to_string(path).map_err(|e| GeneratorError::FileError {
        message: format!("Failed to read file {}: {}", path.display(), e),
    })?;

    let is_yaml = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
        });

    if is_yaml {
        crate::spec_source::yaml_to_json_value(&content).map_err(|error| {
            GeneratorError::FileError {
                message: format!(
                    "Failed to parse schema extension {} as YAML: {}",
                    path.display(),
                    error
                ),
            }
        })
    } else {
        serde_json::from_str(&content).map_err(|error| GeneratorError::FileError {
            message: format!(
                "Failed to parse schema extension {} as JSON: {}",
                path.display(),
                error
            ),
        })
    }
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
            // Special handling for schema objects with oneOf/anyOf variants.
            // Detect which keyword the MAIN spec uses so we preserve it after merging.
            let main_union_keyword = if main_obj.contains_key("oneOf") {
                Some("oneOf")
            } else if main_obj.contains_key("anyOf") {
                Some("anyOf")
            } else {
                None
            };
            if let (Some(main_variants), Some(ext_variants)) = (
                extract_schema_variants(&Value::Object(main_obj.clone())),
                extract_schema_variants(&Value::Object(ext_obj.clone())),
            ) {
                let union_key = main_union_keyword.unwrap_or("oneOf");
                println!(
                    "🔍 Merging union schemas ({union_key}): {} main variants, {} extension variants",
                    main_variants.len(),
                    ext_variants.len()
                );
                // Merge the variant arrays, preserving the original union keyword
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

                // Remove old oneOf/anyOf keys and add merged variants under the original keyword
                main_obj.remove("oneOf");
                main_obj.remove("anyOf");
                main_obj.insert(union_key.to_string(), Value::Array(merged_variants));

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

/// The source identity of a generated schema is distinct from the Rust-facing
/// name eventually allocated to it. Component names are reserved before any
/// traversal, while inline and deep-pointer identities retain enough
/// provenance to reuse their own allocation without impersonating a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InlineUnionKind {
    OneOf,
    AnyOf,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SchemaIdentity {
    Component(String),
    Pointer(String),
    InlineUnionBranch {
        owner_context: String,
        union_kind: InlineUnionKind,
        original_index: usize,
        discriminator: Option<String>,
        fingerprint: String,
    },
    Inline {
        context: String,
        kind: &'static str,
        preferred_name: String,
        fingerprint: String,
    },
}

#[derive(Debug, Default)]
struct SchemaNameRegistry {
    names_by_identity: BTreeMap<SchemaIdentity, String>,
    identities_by_name: BTreeMap<String, SchemaIdentity>,
}

impl SchemaNameRegistry {
    fn with_components(component_names: impl IntoIterator<Item = String>) -> Self {
        let mut registry = Self::default();
        for name in component_names {
            let identity = SchemaIdentity::Component(name.clone());
            registry
                .names_by_identity
                .insert(identity.clone(), name.clone());
            registry.identities_by_name.insert(name, identity);
        }
        registry
    }

    fn component_name(&self, source_name: &str) -> Option<&str> {
        self.names_by_identity
            .get(&SchemaIdentity::Component(source_name.to_string()))
            .map(String::as_str)
    }

    fn allocate(
        &mut self,
        identity: SchemaIdentity,
        preferred_name: &str,
        collision_name: &str,
    ) -> String {
        if let Some(existing) = self.names_by_identity.get(&identity) {
            return existing.clone();
        }

        let allocated = if !self.identities_by_name.contains_key(preferred_name) {
            preferred_name.to_string()
        } else if !self.identities_by_name.contains_key(collision_name) {
            collision_name.to_string()
        } else {
            let hash = stable_schema_identity_hash(&identity);
            let hashed = format!("{collision_name}{hash:016X}");
            if !self.identities_by_name.contains_key(&hashed) {
                hashed
            } else {
                let mut suffix = 2;
                loop {
                    let candidate = format!("{hashed}{suffix}");
                    if !self.identities_by_name.contains_key(&candidate) {
                        break candidate;
                    }
                    suffix += 1;
                }
            }
        };

        self.names_by_identity
            .insert(identity.clone(), allocated.clone());
        self.identities_by_name.insert(allocated.clone(), identity);
        allocated
    }
}

/// Stable FNV-1a rather than `DefaultHasher`, whose output is deliberately not
/// a cross-version contract. This suffix is only a final fallback after both a
/// preferred and human-readable collision name are occupied.
fn stable_schema_identity_hash(identity: &SchemaIdentity) -> u64 {
    let bytes = format!("{identity:?}");
    bytes
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

#[cfg(test)]
mod schema_name_registry_tests {
    use super::{SchemaIdentity, SchemaNameRegistry};

    #[test]
    fn component_reservations_are_independent_of_input_traversal_order() {
        let component_names = ["ModelApi".to_string(), "OutputFormatContainer".to_string()];
        let mut forward = SchemaNameRegistry::with_components(component_names.clone());
        let mut reverse = SchemaNameRegistry::with_components(component_names.into_iter().rev());
        let inline = SchemaIdentity::Inline {
            context: "Model".to_string(),
            kind: "property-object",
            preferred_name: "ModelApi".to_string(),
            fingerprint: r#"{"type":"object"}"#.to_string(),
        };

        assert_eq!(
            forward.allocate(inline.clone(), "ModelApi", "ModelApiInline"),
            "ModelApiInline"
        );
        assert_eq!(
            reverse.allocate(inline, "ModelApi", "ModelApiInline"),
            "ModelApiInline"
        );
        assert_eq!(forward.component_name("ModelApi"), Some("ModelApi"));
        assert_eq!(reverse.component_name("ModelApi"), Some("ModelApi"));
        assert_eq!(
            forward.component_name("OutputFormatContainer"),
            Some("OutputFormatContainer")
        );
        assert_eq!(
            reverse.component_name("OutputFormatContainer"),
            Some("OutputFormatContainer")
        );
    }

    #[test]
    fn an_identity_reuses_its_exact_allocated_name() {
        let mut registry = SchemaNameRegistry::with_components(["ModelApi".to_string()]);
        let inline = SchemaIdentity::Inline {
            context: "Model".to_string(),
            kind: "property-object",
            preferred_name: "ModelApi".to_string(),
            fingerprint: r#"{"type":"object"}"#.to_string(),
        };

        let first = registry.allocate(inline.clone(), "ModelApi", "ModelApiInline");
        let repeated = registry.allocate(inline, "Ignored", "IgnoredInline");

        assert_eq!(first, "ModelApiInline");
        assert_eq!(repeated, first);
        assert_eq!(registry.component_name("ModelApi"), Some("ModelApi"));
    }
}

pub struct SchemaAnalyzer {
    schemas: BTreeMap<String, Schema>,
    resolved_cache: BTreeMap<String, AnalyzedSchema>,
    schema_names: SchemaNameRegistry,
    openapi_spec: Value,
    current_schema_name: Option<String>,
    component_parameters: BTreeMap<String, crate::openapi::Parameter>,
    /// Single chokepoint for `(openapi_type, format)` → Rust-type
    /// decisions (Q2.0). Defaulted when the analyzer is built without a
    /// config; threaded from `GeneratorConfig.types` via
    /// [`Self::with_type_mapper`].
    type_mapper: TypeMapper,
    /// Pointer targets currently being expanded, so a node that references
    /// itself through a pointer stops at a reference instead of recursing.
    resolving_pointers: HashSet<String>,
}

impl SchemaAnalyzer {
    /// The type to emit when a schema gives analysis nothing to work with.
    /// Every `serde_json::Value` that analysis produces goes through here or
    /// [`Self::untyped_value_array`], so a fallback cannot escape the census.
    fn untyped_value(&self, _context: impl Into<String>, reason: UntypedReason) -> SchemaType {
        SchemaType::Untyped {
            shape: UntypedShape::Value,
            reason,
        }
    }

    /// As [`Self::untyped_value`], for an array of unconstrained elements.
    fn untyped_value_array(
        &self,
        _context: impl Into<String>,
        reason: UntypedReason,
    ) -> SchemaType {
        SchemaType::Untyped {
            shape: UntypedShape::ValueArray,
            reason,
        }
    }

    /// The schema currently being analyzed, for finding context.
    fn untyped_context(&self, detail: &str) -> String {
        match (&self.current_schema_name, detail) {
            (Some(schema), "") => schema.clone(),
            (Some(schema), detail) => format!("{schema}.{detail}"),
            (None, "") => "<anonymous>".to_string(),
            (None, detail) => detail.to_string(),
        }
    }

    fn allocate_inline_schema_name(
        &mut self,
        preferred_name: &str,
        collision_name: &str,
        kind: &'static str,
        schema: &Schema,
    ) -> String {
        let identity = SchemaIdentity::Inline {
            context: self
                .current_schema_name
                .clone()
                .unwrap_or_else(|| "<anonymous>".to_string()),
            kind,
            preferred_name: preferred_name.to_string(),
            fingerprint: serde_json::to_string(schema).unwrap_or_else(|_| format!("{schema:?}")),
        };
        self.schema_names
            .allocate(identity, preferred_name, collision_name)
    }

    /// Run nested analysis under the name of the schema that will own the
    /// generated Rust item. Restoring the previous context even on error keeps
    /// sibling paths independent and makes names encode the complete owning
    /// path (`RootWrapperUser`, not a second `RootUser`).
    fn with_schema_context<T>(
        &mut self,
        schema_name: &str,
        analyze: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let previous = self.current_schema_name.replace(schema_name.to_string());
        let result = analyze(self);
        self.current_schema_name = previous;
        result
    }

    fn add_allocated_object_schema(
        &mut self,
        object_type_name: String,
        schema: &Schema,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        let object_type = self.with_schema_context(&object_type_name, |analyzer| {
            analyzer.analyze_object_schema(schema, dependencies)
        })?;
        self.resolved_cache.insert(
            object_type_name.clone(),
            AnalyzedSchema {
                name: object_type_name.clone(),
                original: serde_json::to_value(schema).unwrap_or(Value::Null),
                schema_type: object_type,
                dependencies: dependencies.clone(),
                nullable: false,
                description: schema.details().description.clone(),
                default: None,
            },
        );
        dependencies.insert(object_type_name.clone());
        Ok(SchemaType::Reference {
            target: object_type_name,
        })
    }

    fn allocate_inline_union_branch_name(
        &mut self,
        preferred_name: &str,
        owner_context: &str,
        union_kind: InlineUnionKind,
        original_index: usize,
        discriminator: Option<&str>,
        schema: &Schema,
    ) -> String {
        let identity = SchemaIdentity::InlineUnionBranch {
            owner_context: owner_context.to_string(),
            union_kind,
            original_index,
            discriminator: discriminator.map(str::to_string),
            fingerprint: serde_json::to_string(schema).unwrap_or_else(|_| format!("{schema:?}")),
        };
        self.schema_names
            .allocate(identity, preferred_name, &format!("{preferred_name}Inline"))
    }

    fn allocate_pointer_schema_name(&mut self, pointer: &str, preferred_name: &str) -> String {
        self.schema_names.allocate(
            SchemaIdentity::Pointer(pointer.to_string()),
            preferred_name,
            &format!("{preferred_name}Pointer"),
        )
    }

    fn allocate_synthetic_schema_name(
        &mut self,
        preferred_name: &str,
        collision_name: &str,
        kind: &'static str,
        fingerprint: String,
    ) -> String {
        let identity = SchemaIdentity::Inline {
            context: self
                .current_schema_name
                .clone()
                .unwrap_or_else(|| "<anonymous>".to_string()),
            kind,
            preferred_name: preferred_name.to_string(),
            fingerprint,
        };
        self.schema_names
            .allocate(identity, preferred_name, collision_name)
    }

    fn uses_aws_query_conventions(&self) -> bool {
        self.openapi_spec
            .pointer("/info/x-providerName")
            .and_then(Value::as_str)
            .is_some_and(|provider| provider.eq_ignore_ascii_case("amazonaws.com"))
    }

    /// Construct an analyzer with a default [`TypeMapper`]. Pre-Q2.0
    /// callers (tests, simple bins) use this and get bit-identical
    /// behavior to the pre-refactor code.
    pub fn new(openapi_spec: Value) -> Result<Self> {
        Self::with_type_mapper(openapi_spec, TypeMapper::default())
    }

    /// Construct an analyzer with a caller-supplied [`TypeMapper`]
    /// (built from `GeneratorConfig.types`). The CLI / library entry
    /// points use this so user TOML config drives type generation.
    pub fn with_type_mapper(mut openapi_spec: Value, type_mapper: TypeMapper) -> Result<Self> {
        disambiguate_component_schema_names(&mut openapi_spec);
        let spec: OpenApiSpec = parse_spec_document(&openapi_spec)?;
        let schemas = Self::extract_schemas(&spec)?;

        let component_parameters = spec
            .components
            .as_ref()
            .and_then(|c| c.parameters.as_ref())
            .cloned()
            .unwrap_or_default();
        let schema_names = SchemaNameRegistry::with_components(schemas.keys().cloned());
        Ok(Self {
            schemas,
            resolved_cache: BTreeMap::new(),
            schema_names,
            openapi_spec,
            current_schema_name: None,
            component_parameters,
            type_mapper,
            resolving_pointers: HashSet::new(),
        })
    }

    /// Create a new analyzer with schema extensions merged in (default
    /// type mapper).
    pub fn new_with_extensions(
        openapi_spec: Value,
        extension_paths: &[std::path::PathBuf],
    ) -> Result<Self> {
        let merged_spec = merge_schema_extensions(openapi_spec, extension_paths)?;
        Self::new(merged_spec)
    }

    /// Same as [`Self::new_with_extensions`] but with a caller-supplied
    /// type mapper.
    pub fn new_with_extensions_and_type_mapper(
        openapi_spec: Value,
        extension_paths: &[std::path::PathBuf],
        type_mapper: TypeMapper,
    ) -> Result<Self> {
        let merged_spec = merge_schema_extensions(openapi_spec, extension_paths)?;
        Self::with_type_mapper(merged_spec, type_mapper)
    }

    /// Borrow the analyzer's type mapper. Useful for downstream
    /// inspection (e.g. the dep advisory in Q2.8 reads
    /// `type_mapper().used_features()` after generation).
    pub fn type_mapper(&self) -> &TypeMapper {
        &self.type_mapper
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
                if let Some(items_schema) = schema.details().item_schema() {
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
        // OAS 3.1+ requires only one of `paths`, `webhooks`, or `components`.
        // A document may legitimately have no `components.schemas` (e.g. a
        // webhooks-only or paths-only spec). Return an empty map in that case
        // and let downstream codegen handle "no types to emit" gracefully.
        let schemas = spec.components.as_ref().and_then(|c| c.schemas.as_ref());
        Ok(schemas
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default())
    }

    pub fn analyze(&mut self) -> Result<SchemaAnalysis> {
        let validation_context = ValidationContext {
            openapi_version: self
                .openapi_spec
                .get("openapi")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            json_schema_dialect: self
                .openapi_spec
                .get("jsonSchemaDialect")
                .and_then(Value::as_str)
                .map(str::to_string),
            component_schemas: self
                .openapi_spec
                .pointer("/components/schemas")
                .and_then(Value::as_object)
                .map(|schemas| {
                    schemas
                        .iter()
                        .map(|(name, schema)| (name.clone(), schema.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        };
        let mut analysis = SchemaAnalysis {
            schemas: BTreeMap::new(),
            dependencies: DependencyGraph::new(),
            patterns: DetectedPatterns {
                tagged_enum_schemas: HashSet::new(),
                untagged_enum_schemas: HashSet::new(),
                type_mappings: BTreeMap::new(),
            },
            operations: BTreeMap::new(),
            operation_responses: BTreeMap::new(),
            operation_id_aliases: BTreeMap::new(),
            used_type_features: crate::type_mapping::UsedFeatures::default(),
            enum_extensions: BTreeMap::new(),
            validation_context,
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

        disambiguate_analyzed_schema_names(&mut analysis, &self.schemas);

        // Snapshot the type-mapper's used-features set so the
        // generator can decide which helper modules to emit
        // (e.g. base64_serde for `format: byte`).
        analysis.used_type_features = self.type_mapper.used_features();

        // Q2.6: capture x-enum-varnames / x-enum-descriptions from
        // each enum schema's original JSON. Side-channel keyed by
        // analyzed-schema name so we don't have to extend every
        // SchemaType::StringEnum constructor.
        for (name, analyzed) in &analysis.schemas {
            let enum_value_count = match &analyzed.schema_type {
                SchemaType::StringEnum { values } => values.len(),
                SchemaType::ExtensibleEnum { known_values } => known_values.len(),
                _ => continue,
            };
            if let Some(ext) = extract_enum_extensions(&analyzed.original, enum_value_count, name) {
                analysis.enum_extensions.insert(name.clone(), ext);
            }
        }

        for schema in analysis.schemas.values_mut() {
            normalize_untyped(&mut schema.schema_type, 0);
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

    fn all_variants_have_unique_const_values(&self, variants: &[Schema], field_name: &str) -> bool {
        let mut values = HashSet::new();

        variants.iter().all(|variant| {
            let schema = if let Some(ref_str) = variant.reference() {
                let Some(schema_name) = self.extract_schema_name(ref_str) else {
                    return false;
                };
                let Some(schema) = self.schemas.get(schema_name) else {
                    return false;
                };
                schema
            } else {
                variant
            };

            self.extract_discriminator_value_for_field(schema, field_name)
                .is_some_and(|value| values.insert(value))
        })
    }

    /// True when this branch of an anyOf/oneOf is (or resolves to) an
    /// object — the only kind of schema serde can deserialize via an
    /// internally-tagged enum. False for string/number/bool/array branches
    /// or refs to those, including string-enums.
    ///
    /// Used to detect the "hybrid string-or-object" union pattern (see bug
    /// openapi-generator-dpd) so we can downgrade those unions to
    /// `#[serde(untagged)]`.
    fn branch_resolves_to_object(&self, schema: &Schema) -> bool {
        self.branch_resolves_to_object_inner(schema, &mut HashSet::new())
    }

    fn branch_resolves_to_object_inner(
        &self,
        schema: &Schema,
        visited_refs: &mut HashSet<String>,
    ) -> bool {
        // Follow $ref one hop, then ask the same question of the target.
        if let Some(ref_str) = schema.reference() {
            if !visited_refs.insert(ref_str.to_string()) {
                return false;
            }
            let result = match self
                .extract_schema_name(ref_str)
                .and_then(|n| self.schemas.get(n))
            {
                Some(target) => self.branch_resolves_to_object_inner(target, visited_refs),
                None => false,
            };
            visited_refs.remove(ref_str);
            return result;
        }
        // An allOf wrapper around one scalar/array carrier and neutral
        // annotation siblings remains that non-object carrier. Other allOf
        // shapes are object-like only when at least one meaningful member is.
        if let Schema::AllOf { all_of, .. } = schema {
            if Self::single_non_object_allof_carrier(all_of).is_some() {
                return false;
            }
            return all_of
                .iter()
                .filter(|member| !schema_is_annotation_only(member))
                .any(|member| self.branch_resolves_to_object_inner(member, visited_refs));
        }
        // A nested anyOf/oneOf can carry an object discriminator only when
        // every possible branch is itself object-shaped. Treating the wrapper
        // as unconditionally object-like made scalar carrier unions (such as
        // string-or-number aliases) enter object-only discriminator codegen.
        if let Some(variants) = schema.union_variants() {
            return !variants.is_empty()
                && variants
                    .iter()
                    .all(|variant| self.branch_resolves_to_object_inner(variant, visited_refs));
        }
        if matches!(schema.schema_type(), Some(OpenApiSchemaType::Object)) {
            return true;
        }
        if schema.inferred_type() == Some(OpenApiSchemaType::Object) {
            return true;
        }
        // Anything else (string, integer, number, boolean, array, null,
        // string-enum, etc.) cannot carry a JSON tag field.
        false
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

        // A discriminator is only useful when every branch has a distinct
        // value. Repeated values would generate duplicate serde rename tags,
        // making later branches impossible to deserialize. In that case the
        // caller falls back to an untagged union so nested const fields can
        // participate in matching.
        for candidate in &candidates {
            if self.all_variants_have_unique_const_values(variants, candidate) {
                return Some(candidate.clone());
            }
        }

        None
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

    /// Resolve local component-reference chains when deciding field
    /// nullability. A `$ref` node has no nullable details of its own, but its
    /// target may be `anyOf: [T, null]`, `type: [T, null]`, or OpenAPI 3.0
    /// `nullable: true`.
    fn schema_or_reference_is_nullable(&self, schema: &Schema) -> bool {
        let mut current = schema.clone();
        let mut visited = HashSet::new();
        loop {
            if current.is_nullable_any() {
                return true;
            }
            if let Schema::AllOf { all_of, .. } = &current
                && let Some(carrier) = Self::single_non_object_allof_carrier(all_of)
            {
                current = carrier.clone();
                continue;
            }
            let Some(reference) = current.reference() else {
                return false;
            };
            if !visited.insert(reference.to_string()) {
                return false;
            }
            let Some(target) = self.reference_target_schema(reference) else {
                return false;
            };
            current = target;
        }
    }

    /// Carry schema nullability into positions that do not have
    /// [`PropertyInfo::nullable`] metadata of their own. This is deliberately
    /// applied only by container analyzers: wrapping an ordinary object
    /// property here would conflate a missing field with an explicit JSON
    /// `null` and would double-wrap the generator's field-level `Option<T>`.
    fn nullable_container_value(&self, schema: &Schema, schema_type: SchemaType) -> SchemaType {
        if !self.schema_or_reference_is_nullable(schema)
            || matches!(schema_type, SchemaType::Nullable { .. })
            || matches!(schema.schema_type(), Some(OpenApiSchemaType::Null))
        {
            schema_type
        } else {
            SchemaType::Nullable {
                inner_type: Box::new(schema_type),
            }
        }
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

    fn extract_discriminator_values_for_field(
        &self,
        schema: &Schema,
        field_name: &str,
    ) -> Vec<String> {
        self.extract_discriminator_value_domain_for_field(schema, field_name)
            .unwrap_or_default()
    }

    fn extract_discriminator_value_domain_for_field(
        &self,
        schema: &Schema,
        field_name: &str,
    ) -> Option<Vec<String>> {
        let mut visited_refs = HashSet::new();
        self.discriminator_value_domain(schema, field_name, &mut visited_refs)
    }

    /// Returns the string values admitted for `field_name`, or `None` when
    /// the schema does not constrain that field. An empty domain means the
    /// constraints are contradictory. JSON Schema composition matters here:
    /// `allOf` intersects constraints, while `anyOf`/`oneOf` combine the
    /// alternatives. Flattening all of them into one union allowed explicit
    /// discriminator mappings to manufacture tags rejected by their payload.
    fn discriminator_value_domain(
        &self,
        schema: &Schema,
        field_name: &str,
        visited_refs: &mut HashSet<String>,
    ) -> Option<Vec<String>> {
        if let Some(reference) = schema.reference() {
            if !visited_refs.insert(reference.to_string()) {
                return None;
            }
            let result = self
                .extract_schema_name(reference)
                .and_then(|name| self.schemas.get(name))
                .and_then(|target| {
                    self.discriminator_value_domain(target, field_name, visited_refs)
                });
            visited_refs.remove(reference);
            return result;
        }

        let own_domain = schema
            .details()
            .properties
            .as_ref()
            .and_then(|properties| properties.get(field_name))
            .and_then(|property| self.string_constraint_domain(property, visited_refs));

        let composition_domain = match schema {
            Schema::AllOf { all_of, .. } => {
                let mut domain = None;
                for member in all_of {
                    domain = Self::intersect_optional_domains(
                        domain,
                        self.discriminator_value_domain(member, field_name, visited_refs),
                    );
                }
                domain
            }
            Schema::AnyOf { any_of, .. } => {
                self.union_discriminator_domains(any_of, field_name, visited_refs)
            }
            Schema::OneOf { one_of, .. } => {
                self.union_discriminator_domains(one_of, field_name, visited_refs)
            }
            Schema::Bool(false) => Some(Vec::new()),
            _ => None,
        };

        Self::intersect_optional_domains(own_domain, composition_domain)
    }

    fn union_discriminator_domains(
        &self,
        schemas: &[Schema],
        field_name: &str,
        visited_refs: &mut HashSet<String>,
    ) -> Option<Vec<String>> {
        if schemas.is_empty() {
            return Some(Vec::new());
        }
        let mut domain = Vec::new();
        for schema in schemas {
            let values = self.discriminator_value_domain(schema, field_name, visited_refs)?;
            for value in values {
                Self::push_unique_string(&mut domain, &value);
            }
        }
        Some(domain)
    }

    fn string_constraint_domain(
        &self,
        schema: &Schema,
        visited_refs: &mut HashSet<String>,
    ) -> Option<Vec<String>> {
        if let Some(reference) = schema.reference() {
            if !visited_refs.insert(reference.to_string()) {
                return None;
            }
            let result = self
                .extract_schema_name(reference)
                .and_then(|name| self.schemas.get(name))
                .and_then(|target| self.string_constraint_domain(target, visited_refs));
            visited_refs.remove(reference);
            return result;
        }

        let details = schema.details();
        let mut own_domain = None;
        if let Some(value) = details.const_value.as_ref() {
            own_domain = Self::intersect_optional_domains(
                own_domain,
                Some(value.as_str().into_iter().map(str::to_string).collect()),
            );
        }
        if let Some(enum_values) = &details.enum_values {
            own_domain = Self::intersect_optional_domains(
                own_domain,
                Some(
                    enum_values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                ),
            );
        }
        if details
            .extra
            .get("x-stainless-const")
            .and_then(Value::as_bool)
            == Some(true)
            && let Some(default) = details.default.as_ref().and_then(Value::as_str)
        {
            own_domain =
                Self::intersect_optional_domains(own_domain, Some(vec![default.to_string()]));
        }

        let composition_domain = match schema {
            Schema::AllOf { all_of, .. } => {
                let mut domain = None;
                for member in all_of {
                    domain = Self::intersect_optional_domains(
                        domain,
                        self.string_constraint_domain(member, visited_refs),
                    );
                }
                domain
            }
            Schema::AnyOf { any_of, .. } => {
                self.union_string_constraint_domains(any_of, visited_refs)
            }
            Schema::OneOf { one_of, .. } => {
                self.union_string_constraint_domains(one_of, visited_refs)
            }
            Schema::Bool(false) => Some(Vec::new()),
            _ => None,
        };

        Self::intersect_optional_domains(own_domain, composition_domain)
    }

    fn union_string_constraint_domains(
        &self,
        schemas: &[Schema],
        visited_refs: &mut HashSet<String>,
    ) -> Option<Vec<String>> {
        if schemas.is_empty() {
            return Some(Vec::new());
        }
        let mut domain = Vec::new();
        for schema in schemas {
            let values = self.string_constraint_domain(schema, visited_refs)?;
            for value in values {
                Self::push_unique_string(&mut domain, &value);
            }
        }
        Some(domain)
    }

    fn intersect_optional_domains(
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

    fn push_unique_string(values: &mut Vec<String>, value: &str) {
        if !values.iter().any(|existing| existing == value) {
            values.push(value.to_string());
        }
    }

    fn discriminator_property_presence(&self, schema: &Schema, field_name: &str) -> (bool, bool) {
        self.discriminator_property_presence_inner(schema, field_name, &mut HashSet::new(), 0)
    }

    fn discriminator_property_presence_inner(
        &self,
        schema: &Schema,
        field_name: &str,
        visited_refs: &mut HashSet<String>,
        depth: usize,
    ) -> (bool, bool) {
        if depth > 64 {
            return (false, false);
        }
        if let Some(reference) = schema.reference() {
            if !visited_refs.insert(reference.to_string()) {
                return (false, false);
            }
            let result = self
                .extract_schema_name(reference)
                .and_then(|name| self.schemas.get(name))
                .map(|target| {
                    self.discriminator_property_presence_inner(
                        target,
                        field_name,
                        visited_refs,
                        depth + 1,
                    )
                })
                .unwrap_or((false, false));
            visited_refs.remove(reference);
            return result;
        }

        let details = schema.details();
        let mut declared = details
            .properties
            .as_ref()
            .is_some_and(|properties| properties.contains_key(field_name));
        let mut required = details
            .required
            .as_ref()
            .is_some_and(|names| names.iter().any(|name| name == field_name));

        let members = match schema {
            Schema::AllOf { all_of, .. } => Some(all_of.as_slice()),
            Schema::AnyOf { any_of, .. } => Some(any_of.as_slice()),
            Schema::OneOf { one_of, .. } => Some(one_of.as_slice()),
            _ => None,
        };
        if let Some(members) = members {
            for member in members {
                let (member_declared, member_required) = self
                    .discriminator_property_presence_inner(
                        member,
                        field_name,
                        visited_refs,
                        depth + 1,
                    );
                declared |= member_declared;
                required |= member_required;
            }
        }
        (declared, required)
    }

    fn extract_discriminator_value_for_field(
        &self,
        schema: &Schema,
        field_name: &str,
    ) -> Option<String> {
        self.extract_discriminator_values_for_field(schema, field_name)
            .into_iter()
            .next()
    }

    fn get_any_reference<'a>(&self, schema: &'a Schema) -> Option<&'a str> {
        schema.reference().or_else(|| schema.recursive_reference())
    }

    fn extract_schema_name<'a>(&self, ref_str: &'a str) -> Option<&'a str> {
        if ref_str == "#" {
            return None; // Special case for self-reference
        }

        let parts: Vec<&str> = ref_str.split('/').collect();

        // Standard 3.x pattern: #/components/schemas/{SchemaName}. A longer
        // pointer names a node *inside* the component and must be resolved at
        // that exact JSON Pointer rather than being truncated to the root.
        if parts.len() == 4 && parts[0] == "#" && parts[2] == "schemas" {
            return Some(parts[3]);
        }

        // Swagger 2.0 carry-over: some 3.x specs (Google) still use
        // `#/definitions/{SchemaName}`. Treat it as an alias.
        if parts.len() == 3 && parts[0] == "#" && parts[1] == "definitions" {
            return Some(parts[2]);
        }

        // Other local fragments are JSON Pointers, not component names. Let
        // the exact-pointer resolver handle them before applying the legacy
        // last-segment fallback used by non-pointer reference shapes.
        if ref_str.starts_with("#/") {
            return None;
        }

        // Last-segment fallback for other ref shapes — but only if the
        // segment plausibly names a top-level schema (PascalCase, no digits-
        // only, not a JSON-schema keyword like `schema`/`properties`/`items`).
        // pagerduty has `#/components/parameters/foo/schema`, where the last
        // segment "schema" is a sub-path indicator, not a schema name.
        let last = parts.last()?;
        if last.is_empty()
            || last.chars().all(|c| c.is_ascii_digit())
            || matches!(
                *last,
                "schema" | "properties" | "items" | "additionalProperties"
            )
        {
            return None;
        }
        let first = last.chars().next().unwrap_or(' ');
        if !first.is_ascii_alphabetic() || !first.is_ascii_uppercase() {
            return None;
        }
        Some(last)
    }

    /// Return the exact local schema named by a reference, whether the
    /// reference targets a component root or a node deeper in the document.
    fn reference_target_schema(&self, reference: &str) -> Option<Schema> {
        if let Some(name) = self.extract_schema_name(reference) {
            return self.schemas.get(name).cloned();
        }
        let pointer = reference.strip_prefix('#')?;
        if !pointer.starts_with('/') {
            return None;
        }
        Schema::deserialize(self.openapi_spec.pointer(pointer)?).ok()
    }

    fn analyze_schema(&mut self, schema_name: &str) -> Result<AnalyzedSchema> {
        // Component lookup is provenance-typed: an inline schema can never
        // satisfy this cache request merely because it preferred the same
        // emitted name.
        let emitted_name = self
            .schema_names
            .component_name(schema_name)
            .ok_or_else(|| GeneratorError::UnresolvedReference(schema_name.to_string()))?
            .to_string();
        if let Some(cached) = self.resolved_cache.get(&emitted_name) {
            return Ok(cached.clone());
        }

        // Set current schema name for context
        self.current_schema_name = Some(emitted_name.clone());

        let schema = self
            .schemas
            .get(schema_name)
            .ok_or_else(|| GeneratorError::UnresolvedReference(schema_name.to_string()))?
            .clone();

        // Prevent infinite recursion with placeholder
        self.resolved_cache.insert(
            emitted_name.clone(),
            AnalyzedSchema {
                name: emitted_name.clone(),
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

        let analyzed = self.analyze_schema_value(&schema, &emitted_name)?;

        // Update cache with real result
        self.resolved_cache.insert(emitted_name, analyzed.clone());

        Ok(analyzed)
    }

    fn analyze_schema_value(
        &mut self,
        schema: &Schema,
        schema_name: &str,
    ) -> Result<AnalyzedSchema> {
        let details = schema.details();
        let description = details.description.clone();
        // Retain every OpenAPI nullability spelling on named schemas. Named
        // Rust models are the non-null carrier; reference sites consult this
        // bit to wrap the carrier in Option when the target also admits null.
        let nullable = schema.is_nullable_any();
        let mut dependencies = HashSet::new();

        let schema_type = match schema {
            // `true` admits every value; `false` admits none. Neither leaves
            // anything to generate a type from.
            Schema::Bool(accepts_anything) => self.untyped_value(
                self.untyped_context(""),
                if *accepts_anything {
                    UntypedReason::AnySchema
                } else {
                    UntypedReason::NeverMatches
                },
            ),
            Schema::Reference { reference, .. } => {
                // A ref that names no component schema may still address a node
                // in this document — a parameter's schema, a response body, one
                // member of a composition. Resolve the pointer before giving up
                // and typing the field as opaque JSON.
                match self.extract_schema_name(reference) {
                    Some(name) => {
                        let target = name.to_string();
                        dependencies.insert(target.clone());
                        SchemaType::Reference { target }
                    }
                    None => {
                        let reference = reference.clone();
                        if let Some(resolved) =
                            self.resolve_pointer_schema(&reference, &mut dependencies)?
                        {
                            resolved
                        } else {
                            eprintln!(
                                "⚠️  unresolvable $ref `{}` — typing as serde_json::Value",
                                reference
                            );
                            self.untyped_value(
                                format!("$ref {reference}"),
                                UntypedReason::UnresolvedReference,
                            )
                        }
                    }
                }
            }
            Schema::RecursiveRef { recursive_ref, .. }
            | Schema::DynamicRef {
                dynamic_ref: recursive_ref,
                ..
            } => {
                // Handle recursive / dynamic references. J1: full $dynamicRef
                // resolution against $dynamicAnchor scopes is a follow-up; for
                // now we treat them like recursive refs (self-reference when
                // it's a fragment to the same schema, otherwise resolve via
                // schema name).
                if recursive_ref == "#" {
                    dependencies.insert(schema_name.to_string());
                    SchemaType::Reference {
                        target: schema_name.to_string(),
                    }
                } else {
                    let target = self
                        .extract_schema_name(recursive_ref)
                        .unwrap_or(schema_name)
                        .to_string();
                    dependencies.insert(target.clone());
                    SchemaType::Reference { target }
                }
            }
            Schema::Typed { .. } | Schema::TypedMulti { .. } => {
                if let Some(non_null_types) = schema.non_null_schema_types() {
                    let mut variants = Vec::with_capacity(non_null_types.len());
                    for t in non_null_types {
                        variants.push(self.build_typed_multi_union_variant(
                            t,
                            schema,
                            schema_name,
                            &mut dependencies,
                        )?);
                    }
                    SchemaType::Union {
                        variants,
                        exclusive: false,
                    }
                } else {
                    self.analyze_single_typed_schema(
                        schema,
                        schema_name,
                        details,
                        &mut dependencies,
                    )?
                }
            }
            Schema::AnyOf {
                any_of,
                discriminator,
                ..
            } => {
                if Self::union_only_constrains_requiredness(any_of) {
                    return Ok(AnalyzedSchema {
                        name: schema_name.to_string(),
                        original: serde_json::to_value(schema).unwrap_or(Value::Null),
                        schema_type: self.analyze_empty_union(schema, &mut dependencies)?,
                        dependencies,
                        nullable,
                        description,
                        default: details.default.clone(),
                    });
                }
                if let Some(schema_type) = self.analyze_object_with_variants(
                    schema,
                    any_of,
                    schema_name,
                    &mut dependencies,
                )? {
                    return Ok(AnalyzedSchema {
                        name: schema_name.to_string(),
                        original: serde_json::to_value(schema).unwrap_or(Value::Null),
                        schema_type,
                        dependencies,
                        nullable,
                        description,
                        default: details.default.clone(),
                    });
                }
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
                if one_of.is_empty() {
                    self.analyze_empty_union(schema, &mut dependencies)?
                } else if let Some(schema_type) = self.analyze_object_with_variants(
                    schema,
                    one_of,
                    schema_name,
                    &mut dependencies,
                )? {
                    schema_type
                } else {
                    // Handle oneOf discriminated unions
                    self.analyze_oneof_union(
                        one_of,
                        discriminator.as_ref(),
                        schema_name,
                        &mut dependencies,
                        InlineUnionKind::OneOf,
                        None,
                    )?
                }
            }
            Schema::AllOf { all_of, .. } => {
                // Handle allOf composition (schema inheritance)
                self.analyze_allof_composition(schema, all_of, &mut dependencies)?
            }
            Schema::Untyped { .. } => {
                // Try to infer type from structure
                if let Some(inferred) = schema.inferred_type() {
                    match inferred {
                        OpenApiSchemaType::Object => {
                            if self.should_use_dynamic_json(schema) {
                                self.untyped_value(
                                    self.untyped_context(""),
                                    UntypedReason::OpaqueObject,
                                )
                            } else {
                                self.analyze_object_schema(schema, &mut dependencies)?
                            }
                        }
                        OpenApiSchemaType::String if details.is_string_enum() => {
                            SchemaType::StringEnum {
                                values: details.string_enum_values().unwrap_or_default(),
                            }
                        }
                        // `type: null` admits exactly one value; Rust spells
                        // that `()`, which serde reads from and writes as null.
                        OpenApiSchemaType::Null => SchemaType::Primitive {
                            rust_type: self.type_mapper.null_unit().rust_type,
                            serde_with: None,
                        },
                        _ => self.untyped_value(
                            self.untyped_context(""),
                            UntypedReason::UnsupportedTypeKeyword,
                        ),
                    }
                } else {
                    self.untyped_value(self.untyped_context(""), UntypedReason::AnySchema)
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

    /// Resolve a `Schema::Typed`/`Schema::TypedMulti` schema that carries a
    /// single effective type (the 3.1 nullable shorthand already collapses
    /// to this via `schema_type()`). Proper multi-type unions are handled in
    /// [Self::analyze_schema_value] via
    /// [Self::build_typed_multi_union_variant].
    fn analyze_single_typed_schema(
        &mut self,
        schema: &Schema,
        schema_name: &str,
        details: &crate::openapi::SchemaDetails,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        let primary = schema
            .schema_type()
            .cloned()
            .unwrap_or(OpenApiSchemaType::Object);
        let format = details.format.as_deref();
        Ok(match primary {
            OpenApiSchemaType::String => {
                if let Some(values) = details.string_enum_values() {
                    SchemaType::StringEnum { values }
                } else {
                    let mapped = self.type_mapper.string_format(format);
                    SchemaType::Primitive {
                        rust_type: mapped.rust_type,
                        serde_with: mapped.serde_with,
                    }
                }
            }
            OpenApiSchemaType::Integer => SchemaType::Primitive {
                rust_type: self.integer_rust_type(details),
                serde_with: None,
            },
            OpenApiSchemaType::Number => SchemaType::Primitive {
                rust_type: self.type_mapper.number_format(format).rust_type,
                serde_with: None,
            },
            OpenApiSchemaType::Boolean => SchemaType::Primitive {
                rust_type: self.type_mapper.boolean().rust_type,
                serde_with: None,
            },
            OpenApiSchemaType::Array => {
                self.analyze_array_schema(schema, schema_name, dependencies)?
            }
            OpenApiSchemaType::Object => {
                if self.should_use_dynamic_json(schema) {
                    self.untyped_value(self.untyped_context(""), UntypedReason::OpaqueObject)
                } else {
                    self.analyze_object_schema(schema, dependencies)?
                }
            }
            // `type: null` admits exactly one value. Rust spells that `()`,
            // which serde reads from and writes as `null`.
            OpenApiSchemaType::Null => SchemaType::Primitive {
                rust_type: self.type_mapper.null_unit().rust_type,
                serde_with: None,
            },
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
        // Names hoisted property types after the schema being analyzed, which
        // is what `{Parent}{Property}` reads as in generated code.
        let owner_name = self
            .current_schema_name
            .clone()
            .unwrap_or_else(|| "Inline".to_string());

        if let Some(props) = properties {
            for (prop_name, prop_schema) in props {
                // Check if this property is a union that needs a named type
                let prop_type = if let Schema::AnyOf { any_of, .. } = prop_schema {
                    // The union may sit alongside the property's own
                    // properties, or constrain only which of them are
                    // required — OpenAI's `tool_resources.file_search` is
                    // `{properties: {...}, anyOf: [{required: [a]}, {required: [b]}]}`.
                    if Self::union_only_constrains_requiredness(any_of) {
                        self.analyze_empty_union(prop_schema, dependencies)?
                    } else if let Some(with_variants) = {
                        let variant_owner =
                            format!("{owner_name}{}", self.to_pascal_case(prop_name));
                        self.with_schema_context(&variant_owner, |analyzer| {
                            analyzer.analyze_object_with_variants(
                                prop_schema,
                                any_of,
                                &variant_owner,
                                dependencies,
                            )
                        })?
                    } {
                        with_variants
                    } else if self.should_use_dynamic_json(prop_schema) {
                        // This is a dynamic JSON pattern, use serde_json::Value directly
                        self.untyped_value(
                            self.untyped_context(prop_name),
                            UntypedReason::OpaqueObject,
                        )
                    } else if prop_schema.is_nullable_pattern()
                        && let Some(non_null) = prop_schema.non_null_variant()
                    {
                        // 3.1 idiom: `anyOf: [<schema>, {type: null}]`. The
                        // wrapper has no semantic value beyond nullability;
                        // unwrap to the inner type. Without this, the synthesized
                        // wrapper type collides with the inner $ref's name when
                        // the property name produces a colliding parent context
                        // (e.g. `Step.status` → `StepStatus`, which is also the
                        // referenced component).
                        self.analyze_property_schema_with_context(
                            non_null,
                            Some(prop_name),
                            dependencies,
                        )?
                    } else {
                        // This is an anyOf union in a property - create a named union type
                        // Use the current schema name as context to make the union name unique
                        let context_name = self
                            .current_schema_name
                            .clone()
                            .unwrap_or_else(|| "Unknown".to_string());

                        // Generate a name based on both the schema and property name
                        let prop_pascal = self.to_pascal_case(prop_name);
                        let preferred_union_name = format!("{context_name}{prop_pascal}");
                        let union_type_name = self.allocate_inline_schema_name(
                            &preferred_union_name,
                            &format!("{preferred_union_name}Union2"),
                            "property-anyof",
                            prop_schema,
                        );

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
                                dependencies: schema_type_dependencies(&union_schema_type),
                                schema_type: union_schema_type,
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
                    // 3.1 idiom: `oneOf: [<schema>, {type: null}]`. Same
                    // unwrap as anyOf above — without this, the synthesized
                    // wrapper type collides with the inner $ref's name
                    // (discord's `QuarantineUserAction.metadata` →
                    // `QuarantineUserActionMetadata` clashing with the
                    // referenced `QuarantineUserActionMetadata` schema).
                    if prop_schema.is_nullable_pattern()
                        && let Some(non_null) = prop_schema.non_null_variant()
                    {
                        let unwrapped = self.analyze_property_schema_with_context(
                            non_null,
                            Some(prop_name),
                            dependencies,
                        )?;
                        let owner_name = self
                            .current_schema_name
                            .clone()
                            .unwrap_or_else(|| "Inline".to_string());
                        let unwrapped = self.hoist_inline_property_type(
                            &owner_name,
                            prop_name,
                            unwrapped,
                            dependencies,
                        );
                        let prop_details = prop_schema.details();
                        let prop_nullable = true;
                        let prop_description = prop_details.description.clone();
                        let prop_default = prop_details.default.clone();
                        property_info.insert(
                            prop_name.clone(),
                            PropertyInfo {
                                schema_type: unwrapped,
                                nullable: prop_nullable,
                                description: prop_description,
                                default: prop_default,
                                serde_attrs: Vec::new(),
                                synthesized_required: false,
                                constraints: PropertyConstraints::from_schema_details(prop_details),
                            },
                        );
                        continue;
                    }

                    // Handle oneOf discriminated unions in properties
                    let context_name = self
                        .current_schema_name
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string());
                    let prop_pascal = self.to_pascal_case(prop_name);
                    let preferred_union_name = format!("{context_name}{prop_pascal}");
                    let union_type_name = self.allocate_inline_schema_name(
                        &preferred_union_name,
                        &format!("{preferred_union_name}Union2"),
                        "property-oneof",
                        prop_schema,
                    );

                    // Analyze the discriminated union
                    let union_schema_type = self.analyze_oneof_union(
                        one_of,
                        discriminator.as_ref(),
                        &union_type_name,
                        dependencies,
                        InlineUnionKind::OneOf,
                        None,
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

                let prop_type = self.hoist_inline_property_type(
                    &owner_name,
                    prop_name,
                    prop_type,
                    dependencies,
                );

                let prop_details = prop_schema.details();
                // Every nullability form, via one helper — see is_nullable_any.
                let prop_nullable = self.schema_or_reference_is_nullable(prop_schema);
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
                        synthesized_required: false,
                        constraints: PropertyConstraints::from_schema_details(prop_details),
                    },
                );
            }
        }

        // Q2.3: classify additionalProperties three ways. When the
        // spec gives us a schema we analyze it and emit a typed
        // BTreeMap<String, T>; pre-Q2.3 collapsed both Schema and
        // Boolean(true) to the same untyped map. Toggle:
        //   [generator.types.shape] additional_properties_typed
        // Default true; setting false reverts the schema case to
        // Untyped (current pre-Q2.3 behavior).
        let typed_enabled = self
            .type_mapper
            .config()
            .shape
            .as_ref()
            .and_then(|s| s.additional_properties_typed)
            .unwrap_or(true);

        let untyped_required_property = || PropertyInfo {
            schema_type: SchemaType::Untyped {
                shape: UntypedShape::Value,
                reason: UntypedReason::AnySchema,
            },
            nullable: false,
            description: None,
            default: None,
            serde_attrs: Vec::new(),
            synthesized_required: true,
            constraints: PropertyConstraints::default(),
        };
        let (mut additional_properties, required_additional_property, explicitly_forbidden) =
            match &details.additional_properties {
                Some(crate::openapi::AdditionalProperties::Boolean(true)) => (
                    ObjectAdditionalProperties::Untyped,
                    Some(untyped_required_property()),
                    false,
                ),
                Some(crate::openapi::AdditionalProperties::Boolean(false)) => {
                    (ObjectAdditionalProperties::Forbidden, None, true)
                }
                Some(crate::openapi::AdditionalProperties::Schema(value_schema))
                    if typed_enabled =>
                {
                    let analyzed = self.analyze_property_schema_with_context(
                        value_schema,
                        Some("AdditionalProperty"),
                        dependencies,
                    )?;
                    let nullable_value =
                        self.nullable_container_value(value_schema, analyzed.clone());
                    let value_details = value_schema.details();
                    let required_property = PropertyInfo {
                        schema_type: analyzed.clone(),
                        nullable: self.schema_or_reference_is_nullable(value_schema),
                        description: value_details.description.clone(),
                        // A JSON Schema `default` is an annotation, not permission
                        // to omit a name that `required` says must be present.
                        default: None,
                        serde_attrs: Vec::new(),
                        synthesized_required: true,
                        constraints: PropertyConstraints::from_schema_details(value_details),
                    };
                    (
                        ObjectAdditionalProperties::Typed {
                            value_type: Box::new(nullable_value),
                        },
                        Some(required_property),
                        false,
                    )
                }
                Some(crate::openapi::AdditionalProperties::Schema(_)) => (
                    // typed_enabled = false: degrade both the catch-all map and
                    // any required unknown member to serde_json::Value.
                    ObjectAdditionalProperties::Untyped,
                    Some(untyped_required_property()),
                    false,
                ),
                // JSON Schema and OpenAPI 3.0 define an omitted
                // additionalProperties keyword as accepting any extra value.
                // We retain the historical closed-model shape unless an
                // undeclared required name proves that an open carrier is needed.
                None if Self::object_shape_needs_additional_property_carrier(details) => (
                    ObjectAdditionalProperties::Untyped,
                    Some(untyped_required_property()),
                    false,
                ),
                None => (
                    ObjectAdditionalProperties::Forbidden,
                    Some(untyped_required_property()),
                    false,
                ),
            };

        self.finalize_required_object_members(
            &mut property_info,
            &required,
            &mut additional_properties,
            required_additional_property,
            explicitly_forbidden,
        )?;

        Ok(SchemaType::Object {
            properties: property_info,
            variant: None,
            required,
            additional_properties,
        })
    }

    /// Decide when an omitted `additionalProperties` keyword still needs an
    /// emitted catch-all map. JSON Schema leaves such objects open, but the
    /// generator historically projected them as closed structs. Preserve the
    /// open portion when dropping it can invalidate object-count constraints
    /// or discard keys that the schema itself demonstrates in examples.
    fn object_shape_needs_additional_property_carrier(
        details: &crate::openapi::SchemaDetails,
    ) -> bool {
        if details.min_properties.is_some() || details.max_properties.is_some() {
            return true;
        }

        let declared = details.properties.as_ref();
        let has_undeclared_key = |value: &Value| {
            value.as_object().is_some_and(|object| {
                object
                    .keys()
                    .any(|key| declared.is_none_or(|properties| !properties.contains_key(key)))
            })
        };
        details.example.as_ref().is_some_and(has_undeclared_key)
            || details
                .examples
                .as_ref()
                .is_some_and(|examples| examples.iter().any(has_undeclared_key))
    }

    /// Materialize names asserted by `required` but omitted from `properties`.
    /// A flattened map preserves arbitrary extras, but it cannot express that a
    /// particular wire key must exist, so each such name also needs a normal
    /// required field in the generated struct.
    fn finalize_required_object_members(
        &self,
        properties: &mut BTreeMap<String, PropertyInfo>,
        required: &HashSet<String>,
        additional_properties: &mut ObjectAdditionalProperties,
        required_additional_property: Option<PropertyInfo>,
        explicitly_forbidden: bool,
    ) -> Result<()> {
        let mut missing = required
            .iter()
            .filter(|name| !properties.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        missing.sort();
        if missing.is_empty() {
            return Ok(());
        }

        if explicitly_forbidden {
            let owner = self
                .current_schema_name
                .as_deref()
                .unwrap_or("<inline object>");
            return Err(GeneratorError::InvalidSchema(format!(
                "object schema `{owner}` is unsatisfiable: required member(s) {} are not declared in properties while additionalProperties: false",
                missing
                    .iter()
                    .map(|name| format!("`{name}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        let required_additional_property = required_additional_property.ok_or_else(|| {
            GeneratorError::InvalidSchema(
                "undeclared required members have no additional-properties carrier".to_string(),
            )
        })?;
        for name in missing {
            properties.insert(name, required_additional_property.clone());
        }

        if matches!(additional_properties, ObjectAdditionalProperties::Forbidden) {
            *additional_properties = ObjectAdditionalProperties::Untyped;
        }
        Ok(())
    }

    /// Build one union variant for a genuine `type: [X, Y, ...]` member.
    /// All members of a `TypedMulti` share a single `SchemaDetails`, so
    /// `array`/`object` members carry the *same* `items`/`properties` as
    /// the union schema itself — routing them through `TypeMapper::map`
    /// (as the scalar members are) would discard that shape and collapse
    /// to generic `Vec<serde_json::Value>` / `serde_json::Value`.
    ///
    /// This just properly handles array and object types before passing on to
    /// the type mapper.
    fn build_typed_multi_union_variant(
        &mut self,
        member_type: OpenApiSchemaType,
        schema: &Schema,
        union_type_name: &str,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaRef> {
        match member_type {
            OpenApiSchemaType::Array => {
                let preferred_array_type_name = format!("{union_type_name}Array");
                let array_type_name = self.allocate_inline_schema_name(
                    &preferred_array_type_name,
                    &format!("{preferred_array_type_name}Inline"),
                    "typed-multi-array",
                    schema,
                );
                let array_type =
                    self.analyze_array_schema(schema, &array_type_name, dependencies)?;
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
                dependencies.insert(array_type_name.clone());
                Ok(SchemaRef {
                    target: array_type_name,
                    nullable: false,
                })
            }
            OpenApiSchemaType::Object => {
                let preferred_object_type_name = format!("{union_type_name}Object");
                let object_type_name = self.allocate_inline_schema_name(
                    &preferred_object_type_name,
                    &format!("{preferred_object_type_name}Inline"),
                    "typed-multi-object",
                    schema,
                );
                let object_type = self.add_allocated_object_schema(
                    object_type_name.clone(),
                    schema,
                    dependencies,
                )?;
                let SchemaType::Reference { target } = object_type else {
                    unreachable!("allocated object schemas always return a reference");
                };
                Ok(SchemaRef {
                    target,
                    nullable: false,
                })
            }
            _ => Ok(SchemaRef {
                target: self.openapi_type_to_rust_type(member_type, schema.details()),
                nullable: false,
            }),
        }
    }

    fn analyze_property_schema_with_context(
        &mut self,
        schema: &Schema,
        property_name: Option<&str>,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        // `true` admits every value; `false` admits none.
        if let Schema::Bool(accepts_anything) = schema {
            let reason = if *accepts_anything {
                UntypedReason::AnySchema
            } else {
                UntypedReason::NeverMatches
            };
            return Ok(self.untyped_value(
                self.untyped_context(property_name.unwrap_or_default()),
                reason,
            ));
        }

        if let Some(ref_str) = self.get_any_reference(schema) {
            let target_opt = if ref_str == "#" {
                Some(
                    self.find_recursive_anchor_schema()
                        .unwrap_or_else(|| "UnknownRecursive".to_string()),
                )
            } else {
                self.extract_schema_name(ref_str).map(|s| s.to_string())
            };
            match target_opt {
                Some(target) => {
                    dependencies.insert(target.clone());
                    return Ok(SchemaType::Reference { target });
                }
                None => {
                    // Not a component schema, but possibly still a local
                    // pointer into one: specs reference a parameter's schema
                    // (`#/components/parameters/x/schema`) or a member of a
                    // composition (`#/components/schemas/Tag/allOf/0`).
                    if let Some(resolved) = self.resolve_pointer_schema(ref_str, dependencies)? {
                        return Ok(resolved);
                    }
                    eprintln!(
                        "⚠️  unresolvable $ref `{}` — typing as serde_json::Value",
                        ref_str
                    );
                    return Ok(self.untyped_value(
                        format!("$ref {ref_str}"),
                        UntypedReason::UnresolvedReference,
                    ));
                }
            }
        }

        // Genuine multi-scalar `type: [X, Y]` union (not the 3.1 nullable
        // shorthand `[X, "null"]`, which `schema_type()` already collapses).
        // Give it a named enum, same as an anyOf/oneOf union property below.
        if let Some(non_null_types) = schema.non_null_schema_types() {
            let context_name = self
                .current_schema_name
                .clone()
                .unwrap_or_else(|| "Unknown".to_string());
            let prop_pascal = property_name
                .map(|name| self.to_pascal_case(name))
                .unwrap_or_default();
            let preferred_union_name = format!("{context_name}{prop_pascal}");
            let union_type_name = self.allocate_inline_schema_name(
                &preferred_union_name,
                &format!("{preferred_union_name}Union2"),
                "property-typed-multi",
                schema,
            );

            let details = schema.details();
            let mut variants = Vec::with_capacity(non_null_types.len());
            for t in non_null_types {
                variants.push(self.build_typed_multi_union_variant(
                    t,
                    schema,
                    &union_type_name,
                    dependencies,
                )?);
            }

            self.resolved_cache.insert(
                union_type_name.clone(),
                AnalyzedSchema {
                    name: union_type_name.clone(),
                    original: serde_json::to_value(schema).unwrap_or(Value::Null),
                    schema_type: SchemaType::Union {
                        variants,
                        exclusive: false,
                    },
                    dependencies: HashSet::new(),
                    nullable: false,
                    description: details.description.clone(),
                    default: None,
                },
            );

            dependencies.insert(union_type_name.clone());
            return Ok(SchemaType::Reference {
                target: union_type_name,
            });
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

                        // Generate a candidate name based on both the schema and property context.
                        let primary_name = if let Some(prop_name) = property_name {
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

                        return Ok(self.hoist_inline_string_enum(
                            schema,
                            enum_values,
                            primary_name,
                            dependencies,
                        ));
                    } else {
                        // Property-level string with no enum values:
                        // route through TypeMapper so `format: date-time`
                        // / `uuid` / etc. surface as typed scalars
                        // (chrono::DateTime, uuid::Uuid, …) instead of
                        // collapsing to bare `String`.
                        let mapped = self
                            .type_mapper
                            .string_format(schema.details().format.as_deref());
                        return Ok(SchemaType::Primitive {
                            rust_type: mapped.rust_type,
                            serde_with: mapped.serde_with,
                        });
                    }
                }
                OpenApiSchemaType::Integer | OpenApiSchemaType::Number => {
                    let details = schema.details();
                    let rust_type = self.get_number_rust_type(schema_type.clone(), details);
                    return Ok(SchemaType::Primitive {
                        rust_type,
                        serde_with: None,
                    });
                }
                OpenApiSchemaType::Boolean => {
                    return Ok(SchemaType::Primitive {
                        rust_type: "bool".to_string(),
                        serde_with: None,
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
                        return Ok(self
                            .untyped_value(self.untyped_context(""), UntypedReason::OpaqueObject));
                    }
                    // Inline object in property - create a named schema for it
                    let preferred_object_type_name = if let Some(prop_name) = property_name {
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
                    let object_type_name = self.allocate_inline_schema_name(
                        &preferred_object_type_name,
                        &format!("{preferred_object_type_name}Inline"),
                        "property-object",
                        schema,
                    );

                    return self.add_allocated_object_schema(
                        object_type_name,
                        schema,
                        dependencies,
                    );
                }
                // `type: null` admits exactly one value; Rust spells that
                // `()`, which serde reads from and writes as null.
                OpenApiSchemaType::Null => {
                    return Ok(SchemaType::Primitive {
                        rust_type: self.type_mapper.null_unit().rust_type,
                        serde_with: None,
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
            return Ok(self.untyped_value(self.untyped_context(""), UntypedReason::OpaqueObject));
        }

        // Handle allOf composition patterns
        if let Schema::AllOf { all_of, .. } = schema {
            if let Some(property_name) = property_name {
                let owner = self
                    .current_schema_name
                    .clone()
                    .unwrap_or_else(|| "Inline".to_string());
                let composition_name = format!("{owner}{}", self.to_pascal_case(property_name));
                return self.with_schema_context(&composition_name, |analyzer| {
                    analyzer.analyze_allof_composition(schema, all_of, dependencies)
                });
            }
            return self.analyze_allof_composition(schema, all_of, dependencies);
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
                        let union_name = self.allocate_inline_schema_name(
                            &union_name,
                            &format!("{union_name}Union2"),
                            "fallback-property-oneof",
                            schema,
                        );
                        // This is a oneOf - analyze it properly with potential discriminator
                        let oneof_result = self.analyze_oneof_union(
                            one_of,
                            discriminator.as_ref(),
                            &union_name,
                            dependencies,
                            InlineUnionKind::OneOf,
                            None,
                        )?;

                        // If we got a union type (not discriminated), we need to store it as a named type
                        if let SchemaType::Union { .. } = &oneof_result {
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
                            exclusive: false,
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
                        return Ok(self
                            .untyped_value(self.untyped_context(""), UntypedReason::OpaqueObject));
                    }
                    let owner = self
                        .current_schema_name
                        .clone()
                        .unwrap_or_else(|| "Unknown".to_string());
                    let preferred_name = property_name.map_or_else(
                        || format!("{owner}Object"),
                        |property_name| format!("{owner}{}", self.to_pascal_case(property_name)),
                    );
                    let object_type_name = self.allocate_inline_schema_name(
                        &preferred_name,
                        &format!("{preferred_name}Inline"),
                        "property-object",
                        schema,
                    );
                    return self.add_allocated_object_schema(
                        object_type_name,
                        schema,
                        dependencies,
                    );
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
                            serde_with: None,
                        });
                    }
                }
                _ => {
                    // Handle other inferred types
                    let rust_type = self.openapi_type_to_rust_type(inferred_type, schema.details());
                    return Ok(SchemaType::Primitive {
                        rust_type,
                        serde_with: None,
                    });
                }
            }
        }

        Ok(self.untyped_value(self.untyped_context(""), UntypedReason::AnySchema))
    }

    fn analyze_allof_composition(
        &mut self,
        owner_schema: &Schema,
        all_of_schemas: &[Schema],
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        // A scalar/array carrier intersected only with annotation-only
        // siblings keeps its wire type. This is deliberately narrower than
        // "pick the first non-object": multiple carriers or assertion-bearing
        // siblings are true intersections and must not be guessed at.
        if let Some(carrier) = Self::single_non_object_allof_carrier(all_of_schemas) {
            return self.analyze_property_schema_with_context(carrier, None, dependencies);
        }

        // A reference plus annotation-only siblings is still a direct type
        // alias. AWS-style specs frequently encode property descriptions as
        // `allOf: [$ref, { description: ... }]`; recursively expanding a
        // self-reference in that shape can otherwise recurse forever.
        let referenced_targets = all_of_schemas
            .iter()
            .filter_map(|schema| schema.reference())
            .filter_map(|reference| self.extract_schema_name(reference))
            .collect::<Vec<_>>();
        let only_reference_and_annotations = all_of_schemas
            .iter()
            .all(|schema| schema.reference().is_some() || schema_is_annotation_only(schema));
        if referenced_targets.len() == 1 && only_reference_and_annotations {
            let target = referenced_targets[0];
            dependencies.insert(target.to_string());
            return Ok(SchemaType::Reference {
                target: target.to_string(),
            });
        }

        // A single member composes with nothing: `allOf: [{type: string}]` is
        // that string. Specs write it to hang a description off a scalar or a
        // `$ref`, and merging it as an object would lose the type entirely.
        if let [only] = all_of_schemas
            && !matches!(
                only.schema_type(),
                Some(OpenApiSchemaType::Object) | Some(OpenApiSchemaType::Null)
            )
            && only.details().properties.is_none()
        {
            return self.analyze_property_schema_with_context(only, None, dependencies);
        }

        // AllOf represents schema composition - merge all schemas into one
        let mut merged_properties = BTreeMap::new();
        let mut merged_required = HashSet::new();
        let mut merged_variant = None;
        let mut descriptions = Vec::new();

        // Save the current schema context to restore it when analyzing properties
        let current_context = self.current_schema_name.clone();
        let owner_name = current_context.as_deref().unwrap_or("InlineComposition");

        // `properties` and `required` can be siblings of `allOf` on the owner
        // itself. Seed them before walking members so a real declaration from
        // either side wins over any synthesized required placeholder.
        self.merge_schema_into_properties(
            owner_schema,
            &mut merged_properties,
            &mut merged_required,
            dependencies,
        )?;

        for (member_index, schema) in all_of_schemas.iter().enumerate() {
            match schema {
                Schema::Reference { reference, .. } => {
                    let (analyzed_type, analyzed_name, raw_target) =
                        if let Some(target) = self.extract_schema_name(reference) {
                            dependencies.insert(target.to_string());
                            let analyzed_ref = self.analyze_schema(target)?;
                            (
                                Some(analyzed_ref.schema_type),
                                Some(target.to_string()),
                                self.schemas.get(target).cloned(),
                            )
                        } else {
                            (
                                self.resolve_pointer_schema(reference, dependencies)?,
                                None,
                                self.reference_target_schema(reference),
                            )
                        };

                    let merged = if let Some(analyzed_type) = analyzed_type {
                        self.merge_analyzed_object_properties(
                            &analyzed_type,
                            analyzed_name.as_deref(),
                            &mut merged_properties,
                            &mut merged_required,
                            &mut merged_variant,
                            owner_name,
                        )?
                    } else {
                        false
                    };
                    if !merged && let Some(raw_target) = raw_target {
                        self.merge_schema_into_properties(
                            &raw_target,
                            &mut merged_properties,
                            &mut merged_required,
                            dependencies,
                        )?;
                    }
                }
                Schema::AnyOf { any_of, .. }
                    if Self::union_only_constrains_requiredness(any_of) => {}
                Schema::OneOf { one_of, .. }
                    if Self::union_only_constrains_requiredness(one_of) => {}
                Schema::AnyOf { .. } | Schema::OneOf { .. } => {
                    let preferred_name = format!("{owner_name}AllOfVariant{}", member_index + 1);
                    let variant_name =
                        self.add_inline_schema(&preferred_name, schema, dependencies)?;
                    dependencies.insert(variant_name.clone());
                    let analyzed_type = self
                        .resolved_cache
                        .get(&variant_name)
                        .map(|analyzed| analyzed.schema_type.clone())
                        .ok_or_else(|| {
                            GeneratorError::InvalidSchema(format!(
                                "allOf union member `{variant_name}` was not analyzed"
                            ))
                        })?;
                    if !self.merge_analyzed_object_properties(
                        &analyzed_type,
                        Some(&variant_name),
                        &mut merged_properties,
                        &mut merged_required,
                        &mut merged_variant,
                        owner_name,
                    )? {
                        return Ok(self.untyped_value(
                            self.untyped_context(""),
                            UntypedReason::UnrepresentableComposition,
                        ));
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

        // If we successfully merged properties, reconcile required names only
        // after every allOf sibling has contributed its declarations. Doing it
        // per branch would turn a sibling-declared typed field into an opaque
        // placeholder depending on branch order.
        if !merged_properties.is_empty() || !merged_required.is_empty() || merged_variant.is_some()
        {
            let mut additional_properties = if merged_properties
                .values()
                .any(|property| property.synthesized_required)
            {
                ObjectAdditionalProperties::Untyped
            } else {
                ObjectAdditionalProperties::Forbidden
            };
            self.finalize_required_object_members(
                &mut merged_properties,
                &merged_required,
                &mut additional_properties,
                Some(PropertyInfo {
                    schema_type: SchemaType::Untyped {
                        shape: UntypedShape::Value,
                        reason: UntypedReason::AnySchema,
                    },
                    nullable: false,
                    description: None,
                    default: None,
                    serde_attrs: Vec::new(),
                    synthesized_required: true,
                    constraints: PropertyConstraints::default(),
                }),
                false,
            )?;
            Ok(SchemaType::Object {
                properties: merged_properties,
                required: merged_required,
                additional_properties,
                variant: merged_variant,
            })
        } else {
            let schemas = all_of_schemas
                .iter()
                .filter_map(|schema| {
                    let reference = schema.reference()?;
                    let target = self.extract_schema_name(reference)?;
                    dependencies.insert(target.to_string());
                    Some(SchemaRef {
                        target: target.to_string(),
                        nullable: false,
                    })
                })
                .collect::<Vec<_>>();
            // An empty composition generated an empty struct, silently
            // narrowing scalar/array intersections to `{}`. References to
            // non-object carriers have the same problem. Keep representable
            // object inheritance as Composition; otherwise retain the wire
            // value opaquely instead of inventing an object shape.
            let references_are_objects = all_of_schemas
                .iter()
                .filter(|schema| schema.reference().is_some())
                .all(|schema| self.branch_resolves_to_object(schema));
            if schemas.is_empty() || !references_are_objects {
                Ok(self.untyped_value(
                    self.untyped_context(""),
                    UntypedReason::UnrepresentableComposition,
                ))
            } else {
                Ok(SchemaType::Composition { schemas })
            }
        }
    }

    /// Return the one direct scalar/array carrier in an allOf whose remaining
    /// members are annotation-only. References, objects, unions, boolean
    /// schemas, and multiple assertion-bearing members are intentionally not
    /// collapsed by this narrow recovery path.
    fn single_non_object_allof_carrier(all_of_schemas: &[Schema]) -> Option<&Schema> {
        let mut meaningful = all_of_schemas
            .iter()
            .filter(|schema| !schema_is_annotation_only(schema));
        let carrier = meaningful.next()?;
        if meaningful.next().is_some() {
            return None;
        }

        match carrier {
            Schema::Typed {
                schema_type:
                    OpenApiSchemaType::String
                    | OpenApiSchemaType::Integer
                    | OpenApiSchemaType::Number
                    | OpenApiSchemaType::Boolean
                    | OpenApiSchemaType::Array,
                ..
            } => Some(carrier),
            Schema::TypedMulti { schema_types, .. } => {
                let mut non_null = schema_types
                    .iter()
                    .filter(|schema_type| **schema_type != OpenApiSchemaType::Null);
                let only = non_null.next()?;
                (non_null.next().is_none()
                    && matches!(
                        only,
                        OpenApiSchemaType::String
                            | OpenApiSchemaType::Integer
                            | OpenApiSchemaType::Number
                            | OpenApiSchemaType::Boolean
                            | OpenApiSchemaType::Array
                    ))
                .then_some(carrier)
            }
            _ => None,
        }
    }

    /// Merge the object reached by an analyzed type, following aliases through
    /// the analysis cache. Deep-pointer resolution hoists inline objects and
    /// returns a reference to that cache entry, so allOf composition needs the
    /// same alias-following behavior as a direct component reference.
    fn merge_analyzed_object_properties(
        &mut self,
        schema_type: &SchemaType,
        named_target: Option<&str>,
        merged_properties: &mut BTreeMap<String, PropertyInfo>,
        merged_required: &mut HashSet<String>,
        merged_variant: &mut Option<SchemaRef>,
        owner_name: &str,
    ) -> Result<bool> {
        let mut current = schema_type.clone();
        let mut visited = HashSet::new();
        let mut variant_target = named_target.map(str::to_string);
        loop {
            match current {
                SchemaType::Object {
                    properties,
                    required,
                    variant,
                    ..
                } => {
                    for (name, property) in properties {
                        let keep_declared_sibling =
                            merged_properties.get(&name).is_some_and(|existing| {
                                !existing.synthesized_required && property.synthesized_required
                            });
                        if !keep_declared_sibling {
                            merged_properties.insert(name, property);
                        }
                    }
                    merged_required.extend(required);
                    if let Some(variant) = variant {
                        Self::merge_allof_variant(merged_variant, variant, owner_name)?;
                    }
                    return Ok(true);
                }
                SchemaType::Reference { target } => {
                    if !visited.insert(target.clone()) {
                        return Ok(false);
                    }
                    if variant_target.is_none() {
                        variant_target = Some(target.clone());
                    }
                    current = if let Some(analyzed) = self.resolved_cache.get(&target) {
                        analyzed.schema_type.clone()
                    } else if self.schemas.contains_key(&target) {
                        self.analyze_schema(&target)?.schema_type
                    } else {
                        return Ok(false);
                    };
                }
                SchemaType::Union { .. } | SchemaType::DiscriminatedUnion { .. } => {
                    let Some(target) = variant_target else {
                        return Ok(false);
                    };
                    Self::merge_allof_variant(
                        merged_variant,
                        SchemaRef {
                            target,
                            nullable: false,
                        },
                        owner_name,
                    )?;
                    return Ok(true);
                }
                _ => return Ok(false),
            }
        }
    }

    fn merge_allof_variant(
        merged_variant: &mut Option<SchemaRef>,
        candidate: SchemaRef,
        owner_name: &str,
    ) -> Result<()> {
        match merged_variant {
            Some(existing) if existing.target == candidate.target => Ok(()),
            Some(existing) => Err(GeneratorError::InvalidSchema(format!(
                "allOf object `{owner_name}` intersects multiple union members (`{}` and `{}`), which cannot be represented by one flattened variant",
                existing.target, candidate.target
            ))),
            None => {
                *merged_variant = Some(candidate);
                Ok(())
            }
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
                let owner_name = self
                    .current_schema_name
                    .clone()
                    .unwrap_or_else(|| "Inline".to_string());
                let prop_type = self.hoist_inline_property_type(
                    &owner_name,
                    prop_name,
                    prop_type,
                    dependencies,
                );
                let prop_details = prop_schema.details();

                // Properties merged through allOf composition must go through
                // the same nullability check as plain object properties.
                // Real hits: OpenAI Response.incomplete_details (anyOf-with-null,
                // openapi-generator-bgo) and RunPod Pod.startedAt / Pod.template
                // (3.1 type-array, openapi-generator-dsu) — the latter arrive
                // as `null` from the live API for any pod that hasn't started.
                let nullable = self.schema_or_reference_is_nullable(prop_schema);
                merged_properties.insert(
                    prop_name.clone(),
                    PropertyInfo {
                        schema_type: prop_type,
                        nullable,
                        description: prop_details.description.clone(),
                        default: prop_details.default.clone(),
                        serde_attrs: Vec::new(),
                        synthesized_required: false,
                        constraints: PropertyConstraints::from_schema_details(prop_details),
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
        parent_name: &str,
        dependencies: &mut HashSet<String>,
        union_kind: InlineUnionKind,
        source_indices: Option<&[usize]>,
    ) -> Result<SchemaType> {
        let default_indices = (0..one_of_schemas.len()).collect::<Vec<_>>();
        let source_indices = source_indices
            .filter(|indices| indices.len() == one_of_schemas.len())
            .unwrap_or(&default_indices);

        // Branches may be pointers into other parts of the document.
        let expanded_branches;
        let one_of_schemas = match self.expand_pointer_branches(one_of_schemas) {
            Some(expanded) => {
                expanded_branches = expanded;
                expanded_branches.as_slice()
            }
            None => one_of_schemas,
        };

        // A boolean branch either opens the union up or can never be taken.
        let boolean_resolved;
        let boolean_resolved_indices;
        let one_of_schemas = match Self::resolve_boolean_branches(one_of_schemas) {
            Ok(resolved) => {
                boolean_resolved_indices = one_of_schemas
                    .iter()
                    .zip(source_indices.iter().copied())
                    .filter_map(|(branch, index)| {
                        (!matches!(branch, Schema::Bool(false))).then_some(index)
                    })
                    .collect::<Vec<_>>();
                boolean_resolved = resolved;
                boolean_resolved.as_slice()
            }
            Err(()) => {
                return Ok(self.untyped_value(self.untyped_context(""), UntypedReason::AnySchema));
            }
        };
        let source_indices = boolean_resolved_indices.as_slice();
        if one_of_schemas.is_empty() {
            return Ok(self.untyped_value(self.untyped_context(""), UntypedReason::NeverMatches));
        }

        // A union of one is that one, and branches that differ only in
        // constraints share one Rust type. Both are checked before the shape
        // patterns below, which would otherwise synthesize a union type and
        // then fail to represent it.
        if let [only] = one_of_schemas {
            return self
                .analyze_schema_value(only, parent_name)
                .map(|analyzed| analyzed.schema_type);
        }
        if let Some(shared) = self.shared_branch_type(one_of_schemas) {
            return Ok(shared);
        }

        // Pattern: nullable [Type, null] — return the non-null type directly.
        // The nullable bit is recorded at the property level via is_nullable_pattern().
        if one_of_schemas.len() == 2 {
            let null_count = one_of_schemas
                .iter()
                .filter(|s| matches!(s.schema_type(), Some(OpenApiSchemaType::Null)))
                .count();
            if null_count == 1 {
                if let Some(non_null) = one_of_schemas
                    .iter()
                    .find(|s| !matches!(s.schema_type(), Some(OpenApiSchemaType::Null)))
                {
                    return self
                        .analyze_schema_value(non_null, parent_name)
                        .map(|a| a.schema_type);
                }
            }
        }

        // If there's no discriminator, create an untagged union. A nullable
        // referenced branch must also be structural: JSON null has no object
        // discriminator field for an internally tagged enum to inspect.
        if discriminator.is_none()
            || one_of_schemas
                .iter()
                .any(|schema| self.schema_or_reference_is_nullable(schema))
        {
            // Handle untagged unions (oneOf without discriminator)
            return self.analyze_untagged_oneof_union(
                one_of_schemas,
                parent_name,
                dependencies,
                union_kind,
                source_indices,
                None,
            );
        }

        // Bug openapi-generator-dpd: if any branch resolves to a non-object
        // schema (e.g. a string-enum like ToolChoiceOptions), serde cannot
        // deserialize it via an internally-tagged enum because there is no
        // JSON object to read the tag from. Fall back to an untagged union
        // so the scalar branch can still match.
        if one_of_schemas
            .iter()
            .any(|s| !self.branch_resolves_to_object(s))
        {
            return self.analyze_untagged_oneof_union(
                one_of_schemas,
                parent_name,
                dependencies,
                union_kind,
                source_indices,
                discriminator,
            );
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

        // A contradictory tag domain has no schema-valid discriminator value.
        // Keep the payload structural instead of inventing and serializing a
        // tag that the branch's own JSON Schema rejects.
        if one_of_schemas.iter().any(|branch| {
            self.extract_discriminator_value_domain_for_field(branch, &discriminator_field)
                .is_some_and(|values| values.is_empty())
        }) {
            eprintln!(
                "⚠️  discriminated union `{parent_name}` has a branch with contradictory `{discriminator_field}` constraints; using structural union fallback"
            );
            return self.analyze_untagged_oneof_union(
                one_of_schemas,
                parent_name,
                dependencies,
                union_kind,
                source_indices,
                discriminator,
            );
        }

        let mut variants = Vec::new();
        let mut used_variant_names = std::collections::HashSet::new();

        for (variant_schema, original_index) in
            one_of_schemas.iter().zip(source_indices.iter().copied())
        {
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

                    // Mapping keys are dispatch hints, not schema constraints.
                    // When the target branch constrains the discriminator,
                    // retain only mapping keys admitted by that target.
                    let mut discriminator_values = Vec::new();
                    let allowed_domain = self.schemas.get(&schema_name).and_then(|ref_schema| {
                        self.extract_discriminator_value_domain_for_field(
                            ref_schema,
                            &discriminator_field,
                        )
                    });
                    if let Some(mappings) = discriminator.and_then(|disc| disc.mapping.as_ref()) {
                        for (key, target_ref) in mappings {
                            if target_ref == ref_str
                                || self.extract_schema_name(target_ref)
                                    == Some(schema_name.as_str())
                            {
                                if allowed_domain
                                    .as_ref()
                                    .is_some_and(|allowed| !allowed.contains(key))
                                {
                                    let allowed = allowed_domain
                                        .as_ref()
                                        .map(|values| values.join("`, `"))
                                        .unwrap_or_default();
                                    eprintln!(
                                        "⚠️  discriminator mapping conflict in union `{parent_name}`: key `{key}` targets `{schema_name}` but branch allows `{allowed}`; ignoring mapping key"
                                    );
                                } else {
                                    Self::push_unique_string(&mut discriminator_values, key);
                                }
                            }
                        }
                    }
                    if let Some(allowed_domain) = allowed_domain {
                        for value in allowed_domain {
                            Self::push_unique_string(&mut discriminator_values, &value);
                        }
                    }
                    if discriminator_values.is_empty() {
                        discriminator_values
                            .push(self.generate_discriminator_value_from_name(&schema_name));
                    }
                    let discriminator_value = discriminator_values[0].clone();
                    let (discriminator_field_declared, discriminator_field_required) = self
                        .schemas
                        .get(&schema_name)
                        .map(|schema| {
                            self.discriminator_property_presence(schema, &discriminator_field)
                        })
                        .unwrap_or((false, false));

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
                        preferred_discriminator_values: discriminator_values.clone(),
                        discriminator_values,
                        discriminator_field_declared,
                        discriminator_field_required,
                        schema_ref: ref_str.to_string(),
                    });
                }
            } else {
                // Handle inline schemas in oneOf
                let variant_index = original_index;
                let inline_type_name =
                    self.generate_inline_type_name(variant_schema, variant_index);

                // Inline branches follow the same multi-value rules as
                // referenced branches.
                let mut discriminator_values = Vec::new();
                let allowed_domain = self.extract_discriminator_value_domain_for_field(
                    variant_schema,
                    &discriminator_field,
                );
                if let Some(mappings) = discriminator.and_then(|disc| disc.mapping.as_ref()) {
                    for (key, target_ref) in mappings {
                        if target_ref.contains(&format!("variant_{variant_index}")) {
                            if allowed_domain
                                .as_ref()
                                .is_some_and(|allowed| !allowed.contains(key))
                            {
                                let allowed = allowed_domain
                                    .as_ref()
                                    .map(|values| values.join("`, `"))
                                    .unwrap_or_default();
                                eprintln!(
                                    "⚠️  discriminator mapping conflict in union `{parent_name}`: key `{key}` targets `{inline_type_name}` but branch allows `{allowed}`; ignoring mapping key"
                                );
                            } else {
                                Self::push_unique_string(&mut discriminator_values, key);
                            }
                        }
                    }
                }
                if let Some(allowed_domain) = allowed_domain {
                    for value in allowed_domain {
                        Self::push_unique_string(&mut discriminator_values, &value);
                    }
                }
                if discriminator_values.is_empty() {
                    discriminator_values.push(format!("variant_{variant_index}"));
                }
                let discriminator_value = discriminator_values[0].clone();
                let (discriminator_field_declared, discriminator_field_required) =
                    self.discriminator_property_presence(variant_schema, &discriminator_field);

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

                // Store inline schema before recording the variant so a
                // reserved component collision can return the actual name.
                let inline_type_name = self.add_inline_union_branch_schema(
                    &inline_type_name,
                    variant_schema,
                    dependencies,
                    parent_name,
                    union_kind,
                    original_index,
                    Some(&final_discriminator_value),
                )?;

                variants.push(UnionVariant {
                    rust_name,
                    type_name: inline_type_name.clone(),
                    discriminator_value: final_discriminator_value,
                    preferred_discriminator_values: discriminator_values.clone(),
                    discriminator_values,
                    discriminator_field_declared,
                    discriminator_field_required,
                    schema_ref: format!("inline_{variant_index}"),
                });
            }
        }

        self.disambiguate_shared_discriminator_values(&mut variants, discriminator);

        if variants.is_empty() {
            // If we couldn't create a discriminated union, fall back to an untagged union
            // This handles cases where oneOf contains references or inline schemas without proper discriminators
            let mut union_variants = Vec::new();

            for (variant_schema, original_index) in
                one_of_schemas.iter().zip(source_indices.iter().copied())
            {
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
                    let branch_discriminator = self.inline_union_branch_discriminator_value(
                        variant_schema,
                        discriminator,
                        original_index,
                    );
                    // Handle inline schemas by creating type aliases or using primitive types directly
                    let inline_name = self.generate_context_aware_name(
                        parent_name,
                        "InlineVariant",
                        original_index,
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
                        SchemaType::Primitive { rust_type, .. } => {
                            union_variants.push(SchemaRef {
                                target: rust_type.clone(),
                                nullable: false,
                            });
                        }
                        // For arrays, check if we can determine the item type
                        SchemaType::Array { item_type } => {
                            match item_type.as_ref() {
                                SchemaType::Primitive { rust_type, .. } => {
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
                                    let inline_type_name = self.generate_context_aware_name(
                                        parent_name,
                                        "Variant",
                                        original_index,
                                        None,
                                    );
                                    let inline_type_name = self.add_inline_union_branch_schema(
                                        &inline_type_name,
                                        variant_schema,
                                        dependencies,
                                        parent_name,
                                        union_kind,
                                        original_index,
                                        branch_discriminator.as_deref(),
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
                            let inline_type_name =
                                format!("{}Variant{}", parent_name, original_index + 1);
                            let inline_type_name = self.add_inline_union_branch_schema(
                                &inline_type_name,
                                variant_schema,
                                dependencies,
                                parent_name,
                                union_kind,
                                original_index,
                                branch_discriminator.as_deref(),
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
                    exclusive: matches!(union_kind, InlineUnionKind::OneOf),
                });
            }

            // Only fall back to serde_json::Value if we truly can't analyze the union
            return Ok(self.untyped_value(
                self.untyped_context(""),
                UntypedReason::UnrepresentableUnion,
            ));
        }

        Ok(SchemaType::DiscriminatedUnion {
            discriminator_field,
            variants,
            exclusive: matches!(union_kind, InlineUnionKind::OneOf),
        })
    }

    fn analyze_untagged_oneof_union(
        &mut self,
        one_of_schemas: &[Schema],
        parent_name: &str,
        dependencies: &mut HashSet<String>,
        union_kind: InlineUnionKind,
        source_indices: &[usize],
        discriminator: Option<&Discriminator>,
    ) -> Result<SchemaType> {
        // Drop null-only variants. They mean "may be null" and are surfaced as
        // Option<T> at the property level — including them here produces a junk
        // `SerdeJsonValue(serde_json::Value)` variant. Recognize the equivalent
        // `type`, `const`, and `enum` spellings.
        let filtered: Vec<(usize, &Schema)> = one_of_schemas
            .iter()
            .zip(source_indices.iter().copied())
            .filter_map(|(schema, original_index)| {
                (!schema.is_explicit_null_only()).then_some((original_index, schema))
            })
            .collect();

        // If filtering leaves a single variant, return its analyzed type directly.
        if filtered.len() == 1 {
            return self
                .analyze_schema_value(filtered[0].1, parent_name)
                .map(|a| a.schema_type);
        }

        // Exact, unique branch selection is appropriate for object-only
        // oneOf unions. Mixed scalar/object and unconstrained branches need
        // normal untagged Serde semantics: a `serde_json::Value` alternative,
        // for example, intentionally accepts shapes that a narrower branch
        // can also hydrate.
        let exclusive_object_union = matches!(union_kind, InlineUnionKind::OneOf)
            && filtered
                .iter()
                .all(|(_, schema)| self.branch_resolves_to_object(schema));

        let mut union_variants = Vec::new();

        for (original_index, variant_schema) in filtered {
            // First check if it's a reference or recursive reference
            if let Some(ref_str) = variant_schema.reference() {
                if let Some(schema_name) = self.extract_schema_name(ref_str) {
                    dependencies.insert(schema_name.to_string());
                    union_variants.push(SchemaRef {
                        target: schema_name.to_string(),
                        nullable: self.schema_or_reference_is_nullable(variant_schema),
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
                let branch_discriminator = self.inline_union_branch_discriminator_value(
                    variant_schema,
                    discriminator,
                    original_index,
                );
                // Handle inline schemas by creating type aliases or using primitive types directly
                let inline_name = self.generate_context_aware_name(
                    parent_name,
                    "InlineVariant",
                    original_index,
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
                    SchemaType::Primitive { rust_type, .. } => {
                        union_variants.push(SchemaRef {
                            target: rust_type.clone(),
                            nullable: false,
                        });
                    }
                    // For arrays, check if we can determine the item type
                    SchemaType::Array { item_type } => {
                        match item_type.as_ref() {
                            SchemaType::Primitive { rust_type, .. } => {
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
                                    SchemaType::Primitive { rust_type, .. } => {
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
                                        let inline_type_name = self.generate_context_aware_name(
                                            parent_name,
                                            "Variant",
                                            original_index,
                                            None,
                                        );
                                        let inline_type_name = self
                                            .add_inline_union_branch_schema(
                                                &inline_type_name,
                                                variant_schema,
                                                dependencies,
                                                parent_name,
                                                union_kind,
                                                original_index,
                                                branch_discriminator.as_deref(),
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
                                let inline_type_name = self.generate_context_aware_name(
                                    parent_name,
                                    "Variant",
                                    original_index,
                                    None,
                                );
                                let inline_type_name = self.add_inline_union_branch_schema(
                                    &inline_type_name,
                                    variant_schema,
                                    dependencies,
                                    parent_name,
                                    union_kind,
                                    original_index,
                                    branch_discriminator.as_deref(),
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
                        let inline_type_name = self.generate_context_aware_name(
                            parent_name,
                            "Variant",
                            original_index,
                            None,
                        );
                        let inline_type_name = self.add_inline_union_branch_schema(
                            &inline_type_name,
                            variant_schema,
                            dependencies,
                            parent_name,
                            union_kind,
                            original_index,
                            branch_discriminator.as_deref(),
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
                exclusive: exclusive_object_union,
            });
        }

        // Only fall back to serde_json::Value if we truly can't analyze the union
        Ok(self.untyped_value(
            self.untyped_context(""),
            UntypedReason::UnrepresentableUnion,
        ))
    }

    fn add_inline_schema(
        &mut self,
        type_name: &str,
        schema: &Schema,
        dependencies: &mut HashSet<String>,
    ) -> Result<String> {
        let allocated_name = self.allocate_inline_schema_name(
            type_name,
            &format!("{type_name}Inline"),
            "inline-schema",
            schema,
        );
        self.add_allocated_inline_schema(allocated_name, schema, dependencies)
    }

    #[allow(clippy::too_many_arguments)]
    fn add_inline_union_branch_schema(
        &mut self,
        type_name: &str,
        schema: &Schema,
        dependencies: &mut HashSet<String>,
        owner_context: &str,
        union_kind: InlineUnionKind,
        original_index: usize,
        discriminator: Option<&str>,
    ) -> Result<String> {
        let allocated_name = self.allocate_inline_union_branch_name(
            type_name,
            owner_context,
            union_kind,
            original_index,
            discriminator,
            schema,
        );
        self.add_allocated_inline_schema(allocated_name, schema, dependencies)
    }

    fn add_allocated_inline_schema(
        &mut self,
        allocated_name: String,
        schema: &Schema,
        dependencies: &mut HashSet<String>,
    ) -> Result<String> {
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
                        allocated_name.clone(),
                        AnalyzedSchema {
                            name: allocated_name.clone(),
                            original: serde_json::to_value(schema).unwrap_or(Value::Null),
                            schema_type: SchemaType::Primitive {
                                rust_type,
                                serde_with: None,
                            },
                            dependencies: HashSet::new(),
                            nullable: false,
                            description: schema.details().description.clone(),
                            default: None,
                        },
                    );
                    return Ok(allocated_name);
                }
                _ => {}
            }
        }

        // For non-primitive types, analyze the inline schema and add it to our collection
        // Set current_schema_name so nested inline properties (enums, unions, objects)
        // get named with the correct parent context instead of inheriting a stale name
        let analyzed = self.with_schema_context(&allocated_name, |analyzer| {
            analyzer.analyze_schema_value(schema, &allocated_name)
        })?;

        // Add to resolved cache so it can be generated
        self.resolved_cache.insert(allocated_name.clone(), analyzed);

        // Add dependencies
        if let Some(cached) = self.resolved_cache.get(&allocated_name) {
            for dep in &cached.dependencies {
                dependencies.insert(dep.clone());
            }
        }

        Ok(allocated_name)
    }

    fn inline_union_branch_discriminator_value(
        &self,
        schema: &Schema,
        discriminator: Option<&Discriminator>,
        original_index: usize,
    ) -> Option<String> {
        let discriminator = discriminator?;
        discriminator
            .mapping
            .as_ref()
            .and_then(|mappings| {
                mappings
                    .iter()
                    .find(|(_, target_ref)| {
                        target_ref.contains(&format!("variant_{original_index}"))
                    })
                    .map(|(key, _)| key.clone())
            })
            .or_else(|| {
                Some(self.extract_inline_discriminator_value(
                    schema,
                    &discriminator.property_name,
                    original_index,
                ))
            })
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
        // Q2.0: route through the TypeMapper chokepoint. With the default
        // config this produces bit-identical output to the pre-refactor
        // match; later Q2.* issues add format-aware branches inside
        // TypeMapper without touching this function.
        if openapi_type == OpenApiSchemaType::Integer {
            self.integer_rust_type(details)
        } else {
            self.type_mapper.map(openapi_type, details).rust_type
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

    fn disambiguate_shared_discriminator_values(
        &self,
        variants: &mut [UnionVariant],
        discriminator: Option<&Discriminator>,
    ) {
        let mut owners_by_value: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (index, variant) in variants.iter().enumerate() {
            for value in &variant.discriminator_values {
                owners_by_value
                    .entry(value.clone())
                    .or_default()
                    .push(index);
            }
        }

        let mut removals: Vec<(usize, String)> = Vec::new();
        for (value, candidate_owners) in owners_by_value {
            if candidate_owners.len() < 2 {
                continue;
            }

            let explicitly_mapped_owners: Vec<usize> = discriminator
                .and_then(|disc| disc.mapping.as_ref())
                .and_then(|mappings| mappings.get(&value))
                .map(|target_ref| {
                    candidate_owners
                        .iter()
                        .copied()
                        .filter(|index| {
                            let variant = &variants[*index];
                            target_ref == &variant.schema_ref
                                || self.extract_schema_name(target_ref)
                                    == Some(variant.type_name.as_str())
                                || (variant.schema_ref.starts_with("inline_")
                                    && target_ref.contains(&variant.schema_ref))
                        })
                        .collect()
                })
                .unwrap_or_default();

            let winner = if explicitly_mapped_owners.len() == 1 {
                explicitly_mapped_owners.first().copied()
            } else {
                let mut scored: Vec<(usize, usize)> = candidate_owners
                    .iter()
                    .map(|index| {
                        (
                            *index,
                            Self::discriminator_name_affinity(&variants[*index].type_name, &value),
                        )
                    })
                    .collect();
                scored.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
                match scored.as_slice() {
                    [(index, score), rest @ ..]
                        if *score > 0 && rest.first().is_none_or(|(_, next)| score > next) =>
                    {
                        Some(*index)
                    }
                    _ => None,
                }
            };

            if let Some(winner) = winner {
                for index in candidate_owners {
                    if index != winner && variants[index].preferred_discriminator_values.len() > 1 {
                        removals.push((index, value.clone()));
                    }
                }
            }
        }

        for (index, value) in removals {
            variants[index]
                .preferred_discriminator_values
                .retain(|candidate| candidate != &value);
        }
        for variant in variants {
            if let Some(canonical) = variant.preferred_discriminator_values.first() {
                variant.discriminator_value.clone_from(canonical);
            }
        }
    }

    fn discriminator_name_affinity(schema_name: &str, value: &str) -> usize {
        let schema_compact: String = schema_name
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        let value_compact: String = value
            .chars()
            .filter(|character| character.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        let exact_bonus = usize::from(
            !value_compact.is_empty() && schema_compact.contains(value_compact.as_str()),
        ) * 10;
        let token_matches = value
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|token| !token.is_empty())
            .filter(|token| schema_compact.contains(&token.to_ascii_lowercase()))
            .count();
        exact_bonus + token_matches
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

    /// Register an inline string enum as a named `StringEnum` schema and
    /// return a `Reference` to it. Shared by property-level enums
    /// (`{Schema}{Prop}`) and array-item enums (`{Schema}{Prop}Item`).
    ///
    /// Resolves a name that either matches an existing same-valued
    /// enum (dedup) or doesn't collide with a different one.
    ///
    /// Two distinct inline enums can land on the same primary
    /// candidate when a parent schema has a property like
    /// `type` that recurs at multiple nesting levels — e.g.
    /// Latitude.sh's `plan_data.type = ["plans"]` (the
    /// JSON-API resource type) and
    /// `plan_data.attributes.specs.drives[].type =
    /// ["SSD","HDD","NVME"]` both want to become
    /// `PlanDataType`. We must NOT silently overwrite the
    /// first registration: that breaks deserialization
    /// because both fields end up referencing whichever
    /// enum was processed last.
    ///
    /// Disambiguation strategy: append the PascalCase first
    /// enum value (`PlanDataTypeNVME` vs `PlanDataTypePlans`)
    /// and, if that's also claimed with different values,
    /// fall back to a numeric `_2`, `_3`, … suffix.
    fn hoist_inline_string_enum(
        &mut self,
        schema: &Schema,
        enum_values: Vec<String>,
        primary_name: String,
        dependencies: &mut HashSet<String>,
    ) -> SchemaType {
        let suffix = enum_values
            .first()
            .map(|value| self.to_pascal_case(value))
            .unwrap_or_else(|| "Variant".to_string());
        let collision_name = format!("{primary_name}{suffix}");
        let enum_type_name =
            self.allocate_inline_schema_name(&primary_name, &collision_name, "string-enum", schema);
        let should_insert = !self.resolved_cache.contains_key(&enum_type_name);

        // Store the enum as a named schema if this is the
        // first time we've seen this exact (name, values) pair.
        if should_insert {
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
        SchemaType::Reference {
            target: enum_type_name,
        }
    }

    fn analyze_array_schema(
        &mut self,
        schema: &Schema,
        parent_schema_name: &str,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        let details = schema.details();

        // Positional schemas first: when the spec pins the length, the array is
        // a tuple, and `items` (if present at all) only describes elements that
        // cannot occur.
        if let Some(positions) = details.positional_items() {
            return self.analyze_positional_items(
                positions,
                details,
                parent_schema_name,
                dependencies,
            );
        }

        // Check if items field is present
        if let Some(items_schema) = details.item_schema() {
            let item_type = self.analyze_item_schema(
                items_schema,
                parent_schema_name,
                &format!("{parent_schema_name}Item"),
                dependencies,
            )?;
            let item_type = self.hoist_inline_property_type(
                parent_schema_name,
                "Item",
                item_type,
                dependencies,
            );
            Ok(SchemaType::Array {
                item_type: Box::new(item_type),
            })
        } else {
            // No items specified, fall back to generic array
            Ok(
                self.untyped_value_array(
                    self.untyped_context(""),
                    UntypedReason::ArrayWithoutItems,
                ),
            )
        }
    }

    /// The single Rust type every branch of a union maps to, if there is one.
    ///
    /// Specs routinely spell one type as several branches that differ only in
    /// constraints — Runway declares a URI field as three `string` branches
    /// with different `pattern`s and lengths. Every value that matches any
    /// branch is still a `String`, so the union has an exact Rust type; only
    /// the constraints, which are documentation here, differ. Branches whose
    /// mapped types disagree (a `uri` alongside a plain string) are left alone.
    fn shared_branch_type(&self, branches: &[Schema]) -> Option<SchemaType> {
        let mut mapped: Option<(String, Option<String>)> = None;
        let mut scalar_kind: Option<OpenApiSchemaType> = None;
        let mut formats_agree = true;
        for branch in branches {
            if branch.reference().is_some() {
                return None;
            }
            let details = branch.details();
            if details.enum_values.is_some()
                || details.const_value.is_some()
                || details.properties.is_some()
            {
                return None;
            }
            let scalar = match branch.schema_type()? {
                scalar @ (OpenApiSchemaType::String
                | OpenApiSchemaType::Integer
                | OpenApiSchemaType::Number
                | OpenApiSchemaType::Boolean) => scalar.clone(),
                _ => return None,
            };
            match &scalar_kind {
                Some(existing) if *existing != scalar => return None,
                Some(_) => {}
                None => scalar_kind = Some(scalar.clone()),
            }

            let candidate = self.type_mapper.map(scalar, details);
            let candidate = (candidate.rust_type, candidate.serde_with);
            match &mapped {
                Some(existing) if *existing != candidate => formats_agree = false,
                Some(_) => {}
                None => mapped = Some(candidate),
            }
        }

        if formats_agree {
            return mapped.map(|(rust_type, serde_with)| SchemaType::Primitive {
                rust_type,
                serde_with,
            });
        }

        // Same wire type, different typed-scalar refinements — gcore declares an
        // IP field as `ipv4 | ipv6 | ipv4network | ipv6network`, which map to
        // three different Rust types. No single refinement holds for every
        // value, but the declared type does, so fall back to it rather than to
        // `serde_json::Value`.
        let scalar = scalar_kind?;
        let mapped = self
            .type_mapper
            .map(scalar, &crate::openapi::SchemaDetails::default());
        Some(SchemaType::Primitive {
            rust_type: mapped.rust_type,
            serde_with: mapped.serde_with,
        })
    }

    /// Replace union branches that are local JSON Pointers with the schemas
    /// they name.
    ///
    /// PagerDuty builds a request body from three pointers into a response's
    /// `oneOf`. Each branch is resolvable, but a union whose branches are
    /// unresolvable references has nothing to build variants from, so the whole
    /// union used to degrade to `serde_json::Value`. Expanding is one level
    /// deep, which is all these shapes need.
    fn expand_pointer_branches(&self, branches: &[Schema]) -> Option<Vec<Schema>> {
        let mut expanded = Vec::with_capacity(branches.len());
        let mut changed = false;
        for branch in branches {
            let resolved = branch
                .reference()
                .filter(|reference| self.extract_schema_name(reference).is_none())
                .and_then(|reference| reference.strip_prefix('#'))
                .filter(|pointer| pointer.starts_with('/'))
                .and_then(|pointer| self.openapi_spec.pointer(pointer))
                .and_then(|value| Schema::deserialize(value).ok())
                .filter(|schema| schema.reference().is_none());
            match resolved {
                Some(schema) => {
                    expanded.push(schema);
                    changed = true;
                }
                None => expanded.push(branch.clone()),
            }
        }
        changed.then_some(expanded)
    }

    /// Analyze a schema that declares its own `properties` *and* a union.
    ///
    /// `{properties: {...}, anyOf: [A, B]}` means "these fields, and one of
    /// these shapes" — Cloudflare's DLP entries and OpenAI's `file_search`
    /// resources are written this way. Neither half can be dropped: reading
    /// only the union loses the declared fields, and reading only the object
    /// loses the alternatives, which is why this used to generate
    /// `serde_json::Value`.
    ///
    /// The object is generated as a struct and the union as its own enum, held
    /// in a `#[serde(flatten)]` field. Returns `None` when the schema has no
    /// properties of its own, leaving plain unions to the union analyzers.
    fn analyze_object_with_variants(
        &mut self,
        schema: &Schema,
        branches: &[Schema],
        schema_name: &str,
        dependencies: &mut HashSet<String>,
    ) -> Result<Option<SchemaType>> {
        let details = schema.details();
        if details.properties.as_ref().is_none_or(BTreeMap::is_empty) || branches.is_empty() {
            return Ok(None);
        }
        // A nullable wrapper is not a variant set, and requiredness-only
        // branches are handled before this.
        if schema.is_nullable_pattern() || Self::union_only_constrains_requiredness(branches) {
            return Ok(None);
        }

        let base = self.analyze_object_schema(schema, dependencies)?;
        let SchemaType::Object {
            properties,
            required,
            additional_properties,
            ..
        } = base
        else {
            return Ok(None);
        };

        let preferred_variant_name = format!("{schema_name}Variant");
        let variant_name = self.allocate_inline_schema_name(
            &preferred_variant_name,
            &format!("{preferred_variant_name}Inline"),
            "object-variant",
            schema,
        );
        let variant_type = self.analyze_anyof_union(
            branches,
            schema.discriminator(),
            dependencies,
            &variant_name,
        )?;
        // If the union itself has no representation, keep the object rather
        // than flattening something untyped into it.
        if matches!(variant_type, SchemaType::Untyped { .. }) {
            return Ok(Some(SchemaType::Object {
                properties,
                required,
                additional_properties,
                variant: None,
            }));
        }

        self.resolved_cache.insert(
            variant_name.clone(),
            AnalyzedSchema {
                name: variant_name.clone(),
                original: Value::Null,
                dependencies: schema_type_dependencies(&variant_type),
                schema_type: variant_type,
                nullable: false,
                description: None,
                default: None,
            },
        );
        dependencies.insert(variant_name.clone());

        Ok(Some(SchemaType::Object {
            properties,
            required,
            additional_properties,
            variant: Some(SchemaRef {
                target: variant_name,
                nullable: false,
            }),
        }))
    }

    /// Resolve boolean branches in a union.
    ///
    /// `true` accepts every value, so a union containing it admits everything
    /// and has no narrower type. `false` accepts none, so such a branch can
    /// never be taken and is dropped — `oneOf: [A, false]` is `A`.
    ///
    /// Returns `Err(())` when the union is unconstrained.
    #[allow(clippy::result_unit_err)]
    fn resolve_boolean_branches(branches: &[Schema]) -> std::result::Result<Vec<Schema>, ()> {
        if branches
            .iter()
            .any(|branch| matches!(branch, Schema::Bool(true)))
        {
            return Err(());
        }
        Ok(branches
            .iter()
            .filter(|branch| !matches!(branch, Schema::Bool(false)))
            .cloned()
            .collect())
    }

    /// Whether a union's branches constrain only which properties are
    /// required.
    ///
    /// Cloudflare writes `{properties: {...}, anyOf: [{required: [commit_hash]},
    /// {required: [branch]}]}` to say "one of these two fields must be present".
    /// The branches carry no type of their own, and Rust has no way to express
    /// the alternation, so the schema is the object its properties describe —
    /// with both fields optional — rather than an unrepresentable union.
    fn union_only_constrains_requiredness(branches: &[Schema]) -> bool {
        !branches.is_empty()
            && branches
                .iter()
                .all(Self::schema_only_constrains_requiredness)
    }

    /// Requiredness formulas can nest through `anyOf`/`oneOf` and `not`, as
    /// in protobuf-generated "at most one field" schemas. They constrain
    /// presence but add no payload shape for a Rust field to carry.
    fn schema_only_constrains_requiredness(schema: &Schema) -> bool {
        let keys_are_requiredness_or_annotations = serde_json::to_value(schema)
            .ok()
            .and_then(|value| value.as_object().cloned())
            .is_some_and(|object| {
                object.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "required"
                            | "not"
                            | "anyOf"
                            | "oneOf"
                            | "title"
                            | "description"
                            | "deprecated"
                            | "readOnly"
                            | "writeOnly"
                            | "examples"
                            | "example"
                            | "default"
                            | "externalDocs"
                            | "xml"
                            | "$comment"
                    ) || key.starts_with("x-")
                })
            });
        if !keys_are_requiredness_or_annotations {
            return false;
        }

        match schema {
            Schema::AnyOf { any_of, .. } => {
                !any_of.is_empty() && any_of.iter().all(Self::schema_only_constrains_requiredness)
            }
            Schema::OneOf { one_of, .. } => {
                !one_of.is_empty() && one_of.iter().all(Self::schema_only_constrains_requiredness)
            }
            other => {
                let details = other.details();
                details
                    .required
                    .as_ref()
                    .is_some_and(|required| !required.is_empty())
                    || details
                        .not
                        .as_deref()
                        .is_some_and(Self::schema_only_constrains_requiredness)
            }
        }
    }

    /// Analyze a union whose branch list is empty.
    ///
    /// `oneOf: []` and `anyOf: []` admit every value, so the schema means
    /// whatever its remaining keywords say. Discord ships
    /// `{type: integer, format: int32, oneOf: []}` for several enums; reading
    /// only the empty union throws away a perfectly good `i32`.
    fn analyze_empty_union(
        &mut self,
        schema: &Schema,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        // A union that declares no type of its own is still an object when it
        // carries properties: the branches sit alongside them, not instead of
        // them.
        let Some(declared) = schema
            .declared_type()
            .cloned()
            .or_else(|| schema.inferred_type())
            .or_else(|| {
                schema
                    .details()
                    .properties
                    .is_some()
                    .then_some(OpenApiSchemaType::Object)
            })
        else {
            return Ok(self.untyped_value(self.untyped_context(""), UntypedReason::AnySchema));
        };
        match declared {
            OpenApiSchemaType::Object => self.analyze_object_schema(schema, dependencies),
            OpenApiSchemaType::Array => {
                let context = self
                    .current_schema_name
                    .clone()
                    .unwrap_or_else(|| "Inline".to_string());
                self.analyze_array_schema(schema, &context, dependencies)
            }
            scalar => {
                let mapped = self.type_mapper.map(scalar, schema.details());
                Ok(SchemaType::Primitive {
                    rust_type: mapped.rust_type,
                    serde_with: mapped.serde_with,
                })
            }
        }
    }

    /// Resolve a local `$ref` that points somewhere other than
    /// `#/components/schemas/<name>`.
    ///
    /// A JSON Pointer may address any node in the document, and real specs use
    /// that: PagerDuty references a parameter's schema
    /// (`#/components/parameters/audit_method_type/schema`) and a single member
    /// of another schema's composition (`#/components/schemas/Tag/allOf/0`).
    /// Resolving only the component-schema form left those fields untyped even
    /// though the target is right there in the document.
    ///
    /// The target is analyzed as an inline schema and named after its pointer,
    /// so two references to the same node share one generated type.
    fn resolve_pointer_schema(
        &mut self,
        reference: &str,
        dependencies: &mut HashSet<String>,
    ) -> Result<Option<SchemaType>> {
        let Some(pointer) = reference.strip_prefix('#') else {
            return Ok(None);
        };
        if pointer.is_empty() || !pointer.starts_with('/') {
            return Ok(None);
        }
        let preferred_name = pointer_type_name(pointer);
        if preferred_name.is_empty() {
            return Ok(None);
        }
        let name = self.allocate_pointer_schema_name(pointer, &preferred_name);
        // Already resolved once, or currently being resolved further up the
        // stack: reference the name rather than expanding it again.
        if self.resolved_cache.contains_key(&name) || !self.resolving_pointers.insert(name.clone())
        {
            dependencies.insert(name.clone());
            return Ok(Some(SchemaType::Reference { target: name }));
        }

        let resolved = (|| {
            let value = self.openapi_spec.pointer(pointer)?.clone();
            Schema::deserialize(&value).ok()
        })();
        let Some(schema) = resolved else {
            self.resolving_pointers.remove(&name);
            return Ok(None);
        };

        // Analyze the target as the named schema represented by the pointer.
        // Property analysis invents names from the caller's current context
        // (`ActionObject`, `HolderItem`), which makes two uses of the same
        // pointer diverge and can overwrite recursive targets.
        let saved_context = self.current_schema_name.clone();
        self.current_schema_name = Some(name.clone());
        let analyzed = self.analyze_schema_value(&schema, &name);
        self.current_schema_name = saved_context;
        self.resolving_pointers.remove(&name);
        let analyzed = analyzed?;
        dependencies.extend(analyzed.dependencies.iter().cloned());
        if analyzed.schema_type.renders_inline() {
            return Ok(Some(analyzed.schema_type));
        }

        self.resolved_cache.insert(name.clone(), analyzed);
        dependencies.insert(name.clone());
        Ok(Some(SchemaType::Reference { target: name }))
    }

    /// Give a property type a name when it needs one.
    ///
    /// A struct field can hold a primitive, a reference, an array, or a tuple.
    /// Anything else — a merged `allOf`, an inline object, a union, an inline
    /// enum — has to be generated as its own item, and a field can only reach
    /// it by reference. Analysis used to leave those in place, and the
    /// generator, with nothing it could write, emitted `serde_json::Value`:
    /// the schema was understood and then thrown away at the last step.
    fn hoist_inline_property_type(
        &mut self,
        schema_name: &str,
        property_name: &str,
        schema_type: SchemaType,
        dependencies: &mut HashSet<String>,
    ) -> SchemaType {
        if schema_type.renders_inline() {
            return schema_type;
        }

        use heck::ToPascalCase;

        let preferred_name = format!("{schema_name}{}", property_name.to_pascal_case());
        let hoisted_name = self.allocate_synthetic_schema_name(
            &preferred_name,
            &format!("{preferred_name}Inline"),
            "hoisted-property",
            format!("{schema_type:?}"),
        );
        let hoisted_dependencies = schema_type_dependencies(&schema_type);
        self.resolved_cache.insert(
            hoisted_name.clone(),
            AnalyzedSchema {
                name: hoisted_name.clone(),
                original: Value::Null,
                schema_type,
                dependencies: hoisted_dependencies,
                nullable: false,
                description: None,
                default: None,
            },
        );
        dependencies.insert(hoisted_name.clone());
        SchemaType::Reference {
            target: hoisted_name,
        }
    }

    /// Analyze positional element schemas — 2020-12 `prefixItems` or the
    /// draft-04 `items: [A, B]` tuple form — into the tightest type the spec
    /// justifies.
    ///
    /// Three tiers, because `prefixItems` alone does not bound an array's
    /// length and a Rust tuple is fixed-arity:
    ///
    /// 1. the length is pinned → a tuple, one element per position;
    /// 2. no extras are allowed and every position is the same schema →
    ///    `Vec<T>`, which accepts any permitted length;
    /// 3. otherwise → `Vec<serde_json::Value>`, since a payload may legally
    ///    carry more elements, of other types, than the positions describe.
    fn analyze_positional_items(
        &mut self,
        positions: &[Schema],
        details: &crate::openapi::SchemaDetails,
        parent_schema_name: &str,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        if details.positional_items_are_exact() && !positions.is_empty() {
            let mut element_types = Vec::with_capacity(positions.len());
            for (index, position) in positions.iter().enumerate() {
                let element_type = self.analyze_item_schema(
                    position,
                    parent_schema_name,
                    &format!("{parent_schema_name}Item{}", index + 1),
                    dependencies,
                )?;
                element_types.push(self.hoist_inline_property_type(
                    parent_schema_name,
                    &format!("Item{}", index + 1),
                    element_type,
                    dependencies,
                ));
            }
            return Ok(SchemaType::Tuple { element_types });
        }

        // Analyze a shared position only once, and only when the positions are
        // interchangeable: analyzing every position would hoist a named type
        // per inline object, and tiers 2 and 3 discard all but one of them.
        if details.positional_items_are_closed()
            && let Some(shared) = shared_positional_schema(positions)
        {
            let item_type = self.analyze_item_schema(
                shared,
                parent_schema_name,
                &format!("{parent_schema_name}Item"),
                dependencies,
            )?;
            return Ok(SchemaType::Array {
                item_type: Box::new(item_type),
            });
        }

        Ok(self.untyped_value_array(self.untyped_context(""), UntypedReason::OpenPositionalItems))
    }

    /// Analyze one element schema into its generated type.
    ///
    /// `inline_name` names whatever has to be hoisted out of an inline element
    /// schema — an object, a string enum, a union — so tuple positions can pass
    /// a per-position name. `parent_schema_name` stays the enclosing schema,
    /// which is what a `$recursiveRef: "#"` element resolves to.
    fn analyze_item_schema(
        &mut self,
        items_schema: &Schema,
        parent_schema_name: &str,
        inline_name: &str,
        dependencies: &mut HashSet<String>,
    ) -> Result<SchemaType> {
        let item_type = match items_schema {
            Schema::Bool(accepts_anything) => self.untyped_value(
                self.untyped_context(""),
                if *accepts_anything {
                    UntypedReason::AnySchema
                } else {
                    UntypedReason::NeverMatches
                },
            ),
            Schema::Reference { reference, .. } => {
                // Array of referenced types
                if let Some(target) = self.extract_schema_name(reference) {
                    let target = target.to_string();
                    dependencies.insert(target.clone());
                    SchemaType::Reference { target }
                } else {
                    self.resolve_pointer_schema(reference, dependencies)?
                        .ok_or_else(|| GeneratorError::UnresolvedReference(reference.to_string()))?
                }
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
                    OpenApiSchemaType::String => {
                        // Inline string enum in array items — hoist to a
                        // named enum (`{Parent}Item`) instead of collapsing
                        // to `Vec<String>`.
                        match items_schema
                            .details()
                            .string_enum_values()
                            .filter(|values| !values.is_empty())
                        {
                            Some(values) => self.hoist_inline_string_enum(
                                items_schema,
                                values,
                                inline_name.to_string(),
                                dependencies,
                            ),
                            None => SchemaType::Primitive {
                                rust_type: "String".to_string(),
                                serde_with: None,
                            },
                        }
                    }
                    OpenApiSchemaType::Integer | OpenApiSchemaType::Number => {
                        let details = items_schema.details();
                        let rust_type = self.get_number_rust_type(schema_type.clone(), details);
                        SchemaType::Primitive {
                            rust_type,
                            serde_with: None,
                        }
                    }
                    OpenApiSchemaType::Boolean => SchemaType::Primitive {
                        rust_type: "bool".to_string(),
                        serde_with: None,
                    },
                    OpenApiSchemaType::Object => {
                        // Inline object in array - create a named schema for it
                        let preferred_object_type_name = inline_name.to_string();
                        let object_type_name = self.allocate_inline_schema_name(
                            &preferred_object_type_name,
                            &format!("{preferred_object_type_name}Inline"),
                            "array-item-object",
                            items_schema,
                        );

                        self.add_allocated_object_schema(
                            object_type_name,
                            items_schema,
                            dependencies,
                        )?
                    }
                    OpenApiSchemaType::Array => {
                        // Array of arrays - recursively analyze
                        self.analyze_array_schema(items_schema, parent_schema_name, dependencies)?
                    }
                    _ => self.untyped_value(
                        self.untyped_context(""),
                        UntypedReason::UnsupportedTypeKeyword,
                    ),
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
                        let preferred_union_name = format!("{inline_name}Union");
                        let union_name = self.allocate_inline_schema_name(
                            &preferred_union_name,
                            &format!("{preferred_union_name}Inline"),
                            "array-item-union",
                            items_schema,
                        );

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
                            let preferred_object_type_name = inline_name.to_string();
                            let object_type_name = self.allocate_inline_schema_name(
                                &preferred_object_type_name,
                                &format!("{preferred_object_type_name}Inline"),
                                "array-item-object",
                                items_schema,
                            );

                            self.add_allocated_object_schema(
                                object_type_name,
                                items_schema,
                                dependencies,
                            )?
                        }
                        OpenApiSchemaType::String => {
                            // Typeless (OpenAPI 3.1) enum in array items —
                            // same hoisting as the typed-string arm.
                            match items_schema
                                .details()
                                .string_enum_values()
                                .filter(|values| !values.is_empty())
                            {
                                Some(values) => self.hoist_inline_string_enum(
                                    items_schema,
                                    values,
                                    inline_name.to_string(),
                                    dependencies,
                                ),
                                None => SchemaType::Primitive {
                                    rust_type: "String".to_string(),
                                    serde_with: None,
                                },
                            }
                        }
                        OpenApiSchemaType::Integer | OpenApiSchemaType::Number => {
                            let details = items_schema.details();
                            let rust_type = self.get_number_rust_type(inferred, details);
                            SchemaType::Primitive {
                                rust_type,
                                serde_with: None,
                            }
                        }
                        OpenApiSchemaType::Boolean => SchemaType::Primitive {
                            rust_type: "bool".to_string(),
                            serde_with: None,
                        },
                        // `type: null` admits exactly one value; Rust spells
                        // that `()`, which serde reads from and writes as null.
                        OpenApiSchemaType::Null => SchemaType::Primitive {
                            rust_type: self.type_mapper.null_unit().rust_type,
                            serde_with: None,
                        },
                        _ => self.untyped_value(
                            self.untyped_context(""),
                            UntypedReason::UnsupportedTypeKeyword,
                        ),
                    }
                } else {
                    self.untyped_value(self.untyped_context(""), UntypedReason::AnySchema)
                }
            }
            // Compositions and anything else this match does not special-case
            // go through the general property analyzer, which understands
            // `allOf` merging. The caller hoists whatever comes back if a field
            // cannot hold it directly.
            _ => self.analyze_property_schema_with_context(items_schema, None, dependencies)?,
        };

        Ok(self.nullable_container_value(items_schema, item_type))
    }

    fn get_number_rust_type(
        &self,
        schema_type: OpenApiSchemaType,
        details: &crate::openapi::SchemaDetails,
    ) -> String {
        // Q2.0: delegate to the TypeMapper chokepoint. The fallback for
        // non-numeric inputs is preserved for backwards compatibility
        // (callers in 2025-era code path `Integer | Number` here).
        let format = details.format.as_deref();
        match schema_type {
            OpenApiSchemaType::Integer => self.integer_rust_type(details),
            OpenApiSchemaType::Number => self.type_mapper.number_format(format).rust_type,
            _ => self.type_mapper.dynamic_json().rust_type,
        }
    }

    /// Select the narrow configured integer carrier unless the schema itself
    /// proves that a wider, still exactly serializable value is valid. JSON
    /// Schema's `format` is an annotation, so a contradictory `format: int64`
    /// plus a maximum above i64 must follow the numeric bounds rather than
    /// rejecting source-valid wire values at hydration time.
    fn integer_rust_type(&self, details: &crate::openapi::SchemaDetails) -> String {
        fn integer_value(number: &serde_json::Number) -> Option<i128> {
            number
                .as_i64()
                .map(i128::from)
                .or_else(|| number.as_u64().map(i128::from))
                .or_else(|| {
                    number.as_f64().and_then(|value| {
                        (value.fract() == 0.0
                            && value >= i128::MIN as f64
                            && value <= i128::MAX as f64)
                            .then_some(value as i128)
                    })
                })
        }

        fn below(number: &serde_json::Number, boundary: i128) -> bool {
            integer_value(number).is_some_and(|value| value < boundary)
        }

        fn above(number: &serde_json::Number, boundary: i128) -> bool {
            integer_value(number).is_some_and(|value| value > boundary)
        }

        let explicitly_nonnegative = details
            .minimum
            .as_ref()
            .and_then(integer_value)
            .is_some_and(|value| value >= 0)
            || matches!(
                details.exclusive_minimum,
                Some(crate::openapi::ExclusiveBound::Number(value)) if value >= 0.0
            );

        let annotated_numbers = details
            .const_value
            .iter()
            .chain(details.default.iter())
            .chain(details.example.iter())
            .chain(details.enum_values.iter().flatten())
            .chain(details.examples.iter().flatten())
            .filter_map(Value::as_number)
            .collect::<Vec<_>>();
        let below_i32 = details
            .minimum
            .as_ref()
            .is_some_and(|number| below(number, i128::from(i32::MIN)))
            || annotated_numbers
                .iter()
                .any(|number| below(number, i128::from(i32::MIN)))
            || matches!(
                details.exclusive_minimum,
                Some(crate::openapi::ExclusiveBound::Number(value)) if value < i32::MIN as f64
            );
        let above_i32 = details
            .maximum
            .as_ref()
            .is_some_and(|number| above(number, i128::from(i32::MAX)))
            || annotated_numbers
                .iter()
                .any(|number| above(number, i128::from(i32::MAX)))
            || matches!(
                details.exclusive_maximum,
                Some(crate::openapi::ExclusiveBound::Number(value)) if value > i32::MAX as f64
            );
        let below_i64 = details
            .minimum
            .as_ref()
            .is_some_and(|number| below(number, i128::from(i64::MIN)))
            || annotated_numbers
                .iter()
                .any(|number| below(number, i128::from(i64::MIN)))
            || matches!(
                details.exclusive_minimum,
                Some(crate::openapi::ExclusiveBound::Number(value)) if value < i64::MIN as f64
            );
        let above_i64 = details
            .maximum
            .as_ref()
            .is_some_and(|number| above(number, i128::from(i64::MAX)))
            || annotated_numbers
                .iter()
                .any(|number| above(number, i128::from(i64::MAX)))
            || matches!(
                details.exclusive_maximum,
                Some(crate::openapi::ExclusiveBound::Number(value)) if value >= 9_223_372_036_854_775_808.0
            );
        let below_zero = details
            .minimum
            .as_ref()
            .and_then(integer_value)
            .is_some_and(|value| value < 0)
            || annotated_numbers
                .iter()
                .filter_map(|number| integer_value(number))
                .any(|value| value < 0)
            || matches!(
                details.exclusive_minimum,
                Some(crate::openapi::ExclusiveBound::Number(value)) if value < 0.0
            );
        let above_u32 = details
            .maximum
            .as_ref()
            .is_some_and(|number| above(number, i128::from(u32::MAX)))
            || annotated_numbers
                .iter()
                .any(|number| above(number, i128::from(u32::MAX)))
            || matches!(
                details.exclusive_maximum,
                Some(crate::openapi::ExclusiveBound::Number(value)) if value > u32::MAX as f64
            );

        let configured = self
            .type_mapper
            .integer_format(details.format.as_deref())
            .rust_type;
        match configured.as_str() {
            "i32" if below_i32 || above_i32 => {
                if below_i64 || above_i64 {
                    "i128".to_string()
                } else {
                    "i64".to_string()
                }
            }
            "i64" if below_i64 => "i128".to_string(),
            "i64" if above_i64 && explicitly_nonnegative => "u64".to_string(),
            "i64" if above_i64 => "i128".to_string(),
            "u32" if below_zero => {
                if below_i64 || above_i64 {
                    "i128".to_string()
                } else {
                    "i64".to_string()
                }
            }
            "u32" if above_u32 => "u64".to_string(),
            "u64" if below_zero => "i128".to_string(),
            configured => configured.to_string(),
        }
    }

    fn analyze_anyof_union(
        &mut self,
        any_of_schemas: &[Schema],
        discriminator: Option<&Discriminator>,
        dependencies: &mut HashSet<String>,
        context_name: &str,
    ) -> Result<SchemaType> {
        let original_indices = (0..any_of_schemas.len()).collect::<Vec<_>>();

        // Branches may be pointers into other parts of the document.
        let expanded_branches;
        let any_of_schemas = match self.expand_pointer_branches(any_of_schemas) {
            Some(expanded) => {
                expanded_branches = expanded;
                expanded_branches.as_slice()
            }
            None => any_of_schemas,
        };

        // A boolean branch either opens the union up or can never be taken.
        let boolean_resolved;
        let boolean_resolved_indices;
        let any_of_schemas = match Self::resolve_boolean_branches(any_of_schemas) {
            Ok(resolved) => {
                boolean_resolved_indices = any_of_schemas
                    .iter()
                    .zip(original_indices.iter().copied())
                    .filter_map(|(branch, index)| {
                        (!matches!(branch, Schema::Bool(false))).then_some(index)
                    })
                    .collect::<Vec<_>>();
                boolean_resolved = resolved;
                boolean_resolved.as_slice()
            }
            Err(()) => {
                return Ok(self.untyped_value(self.untyped_context(""), UntypedReason::AnySchema));
            }
        };
        let source_indices = boolean_resolved_indices.as_slice();
        if any_of_schemas.is_empty() {
            return Ok(self.untyped_value(self.untyped_context(""), UntypedReason::NeverMatches));
        }

        // Drop null-only variants. Nullability is surfaced as Option<T> at the
        // property level via is_nullable_any(); leaving the null variant in
        // here would produce a phantom `()` or `serde_json::Value` type alias
        // that the generator can't render. Recognize the equivalent `type`,
        // `const`, and `enum` spellings.
        let filtered_owned: Vec<Schema>;
        let filtered_indices: Vec<usize>;
        let (any_of_schemas, source_indices): (&[Schema], &[usize]) = if any_of_schemas
            .iter()
            .any(Schema::is_explicit_null_only)
        {
            filtered_owned = any_of_schemas
                .iter()
                .filter(|s| !s.is_explicit_null_only())
                .cloned()
                .collect();
            filtered_indices = any_of_schemas
                .iter()
                .zip(source_indices.iter().copied())
                .filter_map(|(schema, index)| (!schema.is_explicit_null_only()).then_some(index))
                .collect();
            if filtered_owned.is_empty() {
                return Ok(self.untyped_value(self.untyped_context(""), UntypedReason::AnySchema));
            }
            if filtered_owned.len() == 1 {
                return self
                    .analyze_schema_value(&filtered_owned[0], context_name)
                    .map(|a| a.schema_type);
            }
            (&filtered_owned, &filtered_indices)
        } else {
            (any_of_schemas, source_indices)
        };

        // A union of one is that one: gcore writes `anyOf: [{allOf: [...]}]`
        // to attach an example to a referenced error schema.
        if let [only] = any_of_schemas {
            return self
                .analyze_schema_value(only, context_name)
                .map(|analyzed| analyzed.schema_type);
        }

        // Branches that differ only in constraints share one Rust type, so
        // there is no union to build. Checked before the shape patterns below,
        // which would otherwise synthesize a union type and give up on it.
        if let Some(shared) = self.shared_branch_type(any_of_schemas) {
            return Ok(shared);
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
                return self.analyze_oneof_union(
                    any_of_schemas,
                    Some(disc),
                    context_name,
                    dependencies,
                    InlineUnionKind::AnyOf,
                    Some(source_indices),
                );
            }

            // Auto-detect implicit discriminator from const fields across all variants
            if let Some(disc_field) = self.detect_discriminator_field(any_of_schemas) {
                return self.analyze_oneof_union(
                    any_of_schemas,
                    Some(&Discriminator {
                        property_name: disc_field,
                        mapping: None,
                        default_mapping: None,
                        extensions: crate::extensions::Extensions::default(),
                    }),
                    context_name,
                    dependencies,
                    InlineUnionKind::AnyOf,
                    Some(source_indices),
                );
            }

            // Create an untagged union for flexible matching
            let mut variants = Vec::new();

            for (schema, original_index) in
                any_of_schemas.iter().zip(source_indices.iter().copied())
            {
                if let Some(ref_str) = schema.reference() {
                    if let Some(target) = self.extract_schema_name(ref_str) {
                        dependencies.insert(target.to_string());
                        variants.push(SchemaRef {
                            target: target.to_string(),
                            nullable: self.schema_or_reference_is_nullable(schema),
                        });
                    }
                } else if matches!(schema.schema_type(), Some(OpenApiSchemaType::Object))
                    || schema.inferred_type() == Some(OpenApiSchemaType::Object)
                {
                    // Generate inline object type for anyOf union
                    let inline_type_name = self.generate_inline_type_name(schema, original_index);

                    // Store inline schema for later analysis and generation
                    let inline_type_name = self.add_inline_union_branch_schema(
                        &inline_type_name,
                        schema,
                        dependencies,
                        context_name,
                        InlineUnionKind::AnyOf,
                        original_index,
                        None,
                    )?;

                    variants.push(SchemaRef {
                        target: inline_type_name,
                        nullable: false,
                    });
                } else if matches!(schema.schema_type(), Some(OpenApiSchemaType::Array)) {
                    // Create a unique name for this array type in the union
                    let preferred_array_type_name =
                        if let Some(items_schema) = schema.details().item_schema() {
                            if let Some(ref_str) = items_schema.reference() {
                                if let Some(item_type_name) = self.extract_schema_name(ref_str) {
                                    dependencies.insert(item_type_name.to_string());
                                    format!("{item_type_name}Array")
                                } else {
                                    self.generate_context_aware_name(
                                        context_name,
                                        "Array",
                                        original_index,
                                        Some(schema),
                                    )
                                }
                            } else {
                                self.generate_context_aware_name(
                                    context_name,
                                    "Array",
                                    original_index,
                                    Some(schema),
                                )
                            }
                        } else {
                            self.generate_context_aware_name(
                                context_name,
                                "Array",
                                original_index,
                                Some(schema),
                            )
                        };
                    let array_type_name = self.allocate_inline_union_branch_name(
                        &preferred_array_type_name,
                        context_name,
                        InlineUnionKind::AnyOf,
                        original_index,
                        None,
                        schema,
                    );
                    // Handle array types in unions by creating a type alias.
                    let array_type =
                        self.analyze_array_schema(schema, context_name, dependencies)?;

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
                    // Q2.7: when `primitive_unions` is on (default),
                    // emit the Rust type directly as the variant
                    // target — matches `analyze_untagged_oneof_union`
                    // and produces a clean
                    //   #[serde(untagged)] pub enum Foo { String(String), Integer(i64) }
                    // Pre-Q2.7 / opt-out emits a type alias per
                    // primitive (`pub type FooString = String`) and
                    // references the alias in the variant — works
                    // but adds noise.
                    let primitive_unions = self
                        .type_mapper
                        .config_shape_primitive_unions()
                        .unwrap_or(true);

                    if primitive_unions {
                        variants.push(SchemaRef {
                            target: self
                                .openapi_type_to_rust_type(schema_type.clone(), schema.details()),
                            nullable: false,
                        });
                    } else {
                        let preferred_inline_type_name = match schema_type {
                            OpenApiSchemaType::String => {
                                if original_index == 0 {
                                    format!("{context_name}String")
                                } else {
                                    format!("{context_name}StringVariant{original_index}")
                                }
                            }
                            OpenApiSchemaType::Number => {
                                if original_index == 0 {
                                    format!("{context_name}Number")
                                } else {
                                    format!("{context_name}NumberVariant{original_index}")
                                }
                            }
                            OpenApiSchemaType::Integer => {
                                if original_index == 0 {
                                    format!("{context_name}Integer")
                                } else {
                                    format!("{context_name}IntegerVariant{original_index}")
                                }
                            }
                            OpenApiSchemaType::Boolean => {
                                if original_index == 0 {
                                    format!("{context_name}Boolean")
                                } else {
                                    format!("{context_name}BooleanVariant{original_index}")
                                }
                            }
                            _ => format!("{context_name}Variant{original_index}"),
                        };
                        let inline_type_name = self.allocate_inline_union_branch_name(
                            &preferred_inline_type_name,
                            context_name,
                            InlineUnionKind::AnyOf,
                            original_index,
                            None,
                            schema,
                        );

                        let rust_type =
                            self.openapi_type_to_rust_type(schema_type.clone(), schema.details());

                        self.resolved_cache.insert(
                            inline_type_name.clone(),
                            AnalyzedSchema {
                                name: inline_type_name.clone(),
                                original: serde_json::to_value(schema).unwrap_or(Value::Null),
                                schema_type: SchemaType::Primitive {
                                    rust_type,
                                    serde_with: None,
                                },
                                dependencies: HashSet::new(),
                                nullable: false,
                                description: schema.details().description.clone(),
                                default: None,
                            },
                        );

                        dependencies.insert(inline_type_name.clone());

                        variants.push(SchemaRef {
                            target: inline_type_name,
                            nullable: false,
                        });
                    }
                } else {
                    // A composition can itself be one branch of an outer
                    // union, for example `anyOf: [{ oneOf: [...],
                    // discriminator: ... }, { type: string }]`. It has no
                    // direct `type`, so the arms above used to silently drop
                    // it and leave the generated Rust union unable to hydrate
                    // schema-valid object input. Hoist the branch and let the
                    // normal analyzer preserve its nested composition.
                    let inline_type_name = self.generate_context_aware_name(
                        context_name,
                        "InlineVariant",
                        original_index,
                        Some(schema),
                    );
                    let inline_type_name = self.add_inline_union_branch_schema(
                        &inline_type_name,
                        schema,
                        dependencies,
                        context_name,
                        InlineUnionKind::AnyOf,
                        original_index,
                        None,
                    )?;
                    dependencies.insert(inline_type_name.clone());
                    variants.push(SchemaRef {
                        target: inline_type_name,
                        nullable: false,
                    });
                }
            }

            if !variants.is_empty() {
                return Ok(SchemaType::Union {
                    variants,
                    exclusive: false,
                });
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
                // A branch may enumerate its values (`enum: [...]`), pin one
                // (`const`), or accept any string. The last is what makes the
                // union extensible rather than closed: "one of these, or
                // anything else" is exactly `ExtensibleEnum`.
                match schema
                    .details()
                    .string_enum_values()
                    .filter(|values| !values.is_empty())
                {
                    Some(values) => {
                        for value in values {
                            if !enum_values.contains(&value) {
                                enum_values.push(value);
                            }
                        }
                    }
                    None => {
                        if matches!(schema.schema_type(), Some(OpenApiSchemaType::String)) {
                            has_open_string = true;
                        }
                    }
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
        Ok(self.untyped_value(
            self.untyped_context(""),
            UntypedReason::UnrepresentableUnion,
        ))
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

        // An explicit additionalProperties policy is structural even when no
        // named properties exist. `true`/a schema needs a map carrier, while
        // `false` is a closed empty object (GitHub's `empty-object`) and must
        // not become `serde_json::Value`, which would match every oneOf branch.
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
            // Check for constraints that would make this a structured type.
            // After J5–J8, these are typed fields rather than `extra` lookups.
            let has_structural_constraints = details
                .required
                .as_ref()
                .map(|req| !req.is_empty())
                .unwrap_or(false)
                || details.pattern_properties.is_some()
                || details.property_names.is_some()
                || details.min_properties.is_some()
                || details.max_properties.is_some()
                || details.dependent_required.is_some()
                || details.dependent_schemas.is_some()
                || details.if_schema.is_some()
                || details.then_schema.is_some()
                || details.else_schema.is_some();

            return !has_structural_constraints;
        }

        false
    }

    /// Check whether the object declares any explicit additional-properties policy.
    fn has_explicit_additional_properties(&self, schema: &Schema) -> bool {
        let details = schema.details();
        details.additional_properties.is_some()
    }

    /// Analyze OpenAPI operations to extract request/response schemas
    fn analyze_operations(&mut self, analysis: &mut SchemaAnalysis) -> Result<()> {
        let spec: crate::openapi::OpenApiSpec = parse_spec_document(&self.openapi_spec)?;
        // Operation IDs are emitted into one Rust module, so collision
        // detection spans paths and webhooks. Index their canonical Rust type
        // names once instead of re-canonicalizing every previously analyzed
        // operation for every new endpoint.
        let mut canonical_operation_ids = HashSet::new();

        if let Some(paths) = &spec.paths {
            for (path, path_item) in paths {
                // H11: Path Item may be a $ref to components/pathItems. Resolve here.
                let resolved = self.resolve_path_item(path_item, &spec)?;
                let pi: &crate::openapi::PathItem = resolved.as_ref().unwrap_or(path_item);
                self.ingest_path_item_operations(path, pi, analysis, &mut canonical_operation_ids)?;
            }
        }
        // T4: walk webhooks the same way as paths. Per OAS 3.1+, webhooks are
        // server→consumer callbacks: their request bodies describe payloads
        // the *server* sends *to* the consumer. We currently emit them as
        // ordinary operations so their request/response types land in the
        // generated client; a future bead may add a typed Webhook enum and
        // dispatcher.
        if let Some(webhooks) = &spec.webhooks {
            for (name, path_item) in webhooks {
                let synthetic_path = format!("/__webhook__/{name}");
                self.ingest_path_item_operations(
                    &synthetic_path,
                    path_item,
                    analysis,
                    &mut canonical_operation_ids,
                )?;
            }
        }
        Ok(())
    }

    /// H11: Resolve a Path Item's `$ref` (3.1+ allows them) against
    /// `components/pathItems`. Returns Some(resolved) when a ref was followed,
    /// or None when the input is already inline.
    fn resolve_path_item(
        &self,
        path_item: &crate::openapi::PathItem,
        spec: &crate::openapi::OpenApiSpec,
    ) -> Result<Option<crate::openapi::PathItem>> {
        let Some(reference) = &path_item.reference else {
            return Ok(None);
        };
        let target_name = reference
            .strip_prefix("#/components/pathItems/")
            .ok_or_else(|| {
                GeneratorError::UnresolvedReference(format!(
                    "Path Item $ref must point at #/components/pathItems/{{name}}, got {reference}"
                ))
            })?;
        let pi = spec
            .components
            .as_ref()
            .and_then(|c| c.path_items.as_ref())
            .and_then(|map| map.get(target_name))
            .ok_or_else(|| {
                GeneratorError::UnresolvedReference(format!(
                    "Path Item ref {reference} not found in components/pathItems"
                ))
            })?;
        Ok(Some(pi.clone()))
    }

    fn ingest_path_item_operations(
        &mut self,
        path: &str,
        path_item: &crate::openapi::PathItem,
        analysis: &mut SchemaAnalysis,
        canonical_operation_ids: &mut HashSet<String>,
    ) -> Result<()> {
        for (method, operation) in path_item.operations() {
            // Generate operation ID if missing.
            let raw_operation_id = operation
                .operation_id
                .clone()
                .unwrap_or_else(|| Self::generate_operation_id(method, path));

            // T6: detect operationId collisions. Per the OAS spec these MUST
            // be unique, but real-world specs (arcade, cal-com, telnyx,
            // val-town, …) frequently aren't. Auto-disambiguate by suffixing
            // with the method, then a counter, and warn.
            //
            // The collision key is the PascalCased form so that case-only
            // differences (telnyx has `getMdrUsageReports` AND
            // `GetMdrUsageReports`) collide too — otherwise codegen would
            // produce two `GetMdrUsageReportsApiError` enums in the same
            // module.
            let operation_id = if canonical_operation_ids
                .contains(&Self::canonical_operation_id(&raw_operation_id))
            {
                let method_lower = method.to_lowercase();
                let mut candidate = format!("{}_{}", raw_operation_id, method_lower);
                let mut suffix = 2;
                while canonical_operation_ids.contains(&Self::canonical_operation_id(&candidate)) {
                    candidate = format!("{}_{}_{}", raw_operation_id, method_lower, suffix);
                    suffix += 1;
                }
                eprintln!(
                    "⚠️  duplicate operationId `{}` at `{} {}` — disambiguated to `{}`",
                    raw_operation_id, method, path, candidate
                );
                candidate
            } else {
                raw_operation_id.clone()
            };

            let (op_info, responses) = self.analyze_single_operation(
                &operation_id,
                method,
                path,
                operation,
                path_item.parameters.as_ref(),
                analysis,
            )?;
            analysis
                .operation_id_aliases
                .entry(raw_operation_id)
                .or_default()
                .push(operation_id.clone());
            canonical_operation_ids.insert(Self::canonical_operation_id(&operation_id));
            analysis
                .operation_responses
                .insert(operation_id.clone(), responses);
            analysis.operations.insert(operation_id, op_info);
        }
        Ok(())
    }

    fn canonical_operation_id(operation_id: &str) -> String {
        use heck::ToPascalCase;
        operation_id.replace('.', "_").to_pascal_case()
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
        path_item_parameters: Option<&Vec<crate::openapi::Parameter>>,
        _analysis: &mut SchemaAnalysis,
    ) -> Result<(OperationInfo, BTreeMap<String, OperationResponse>)> {
        let raw_path_item = self
            .openapi_spec
            .get("paths")
            .and_then(|paths| paths.get(path))
            .cloned();
        let raw_operation = raw_path_item
            .as_ref()
            .and_then(|path_item| path_item.get(method.to_ascii_lowercase()))
            .cloned();
        let request_body = operation
            .request_body
            .as_ref()
            .map(|request_body| self.resolve_request_body(request_body))
            .transpose()?;
        let mut op_info = OperationInfo {
            operation_id: operation_id.to_string(),
            method: method.to_uppercase(),
            path: normalize_operation_path(path),
            summary: operation.summary.clone(),
            description: operation.description.clone(),
            request_body: None,
            // Per OAS 3.x §"Request Body Object", `required` defaults to false.
            request_body_required: request_body
                .as_ref()
                .and_then(|rb| rb.required)
                .unwrap_or(false),
            response_schemas: BTreeMap::new(),
            parameters: Vec::new(),
            supports_streaming: false, // Will be determined by StreamingConfig, not spec
            stream_parameter: None,    // Will be determined by StreamingConfig, not spec
            tags: operation.tags.clone().unwrap_or_default(),
        };
        let mut operation_responses = BTreeMap::new();

        // Extract request body schema with content-type awareness
        if let Some(request_body) = &request_body {
            use crate::openapi::{
                is_binary_media_type, is_form_urlencoded_media_type, is_json_media_type,
                media_type_essence,
            };
            if let Some((content_type, maybe_schema)) = request_body.best_content() {
                op_info.request_body = if is_json_media_type(content_type) {
                    match maybe_schema {
                        Some(s) => {
                            let validation_schema = self
                                .raw_request_body_schema(raw_operation.as_ref(), content_type)
                                .unwrap_or(
                                    serde_json::to_value(s).map_err(GeneratorError::ParseError)?,
                                );
                            Some(
                                self.resolve_or_inline_schema(s, operation_id, "Request")
                                    .map(|name| RequestBodyContent::Json {
                                        schema_name: name,
                                        media_type: content_type.to_string(),
                                        validation_schema,
                                    })?,
                            )
                        }
                        None => Some(RequestBodyContent::SchemaLess {
                            media_type: content_type.to_string(),
                        }),
                    }
                } else if is_form_urlencoded_media_type(content_type) {
                    match maybe_schema {
                        Some(s) => {
                            let validation_schema = self
                                .raw_request_body_schema(raw_operation.as_ref(), content_type)
                                .unwrap_or(
                                    serde_json::to_value(s).map_err(GeneratorError::ParseError)?,
                                );
                            Some(
                                self.resolve_or_inline_schema(s, operation_id, "Request")
                                    .map(|name| RequestBodyContent::FormUrlEncoded {
                                        schema_name: name,
                                        media_type: content_type.to_string(),
                                        validation_schema,
                                    })?,
                            )
                        }
                        None => Some(RequestBodyContent::SchemaLess {
                            media_type: content_type.to_string(),
                        }),
                    }
                } else if media_type_essence(content_type)
                    .eq_ignore_ascii_case("multipart/form-data")
                {
                    match maybe_schema {
                        Some(schema) => {
                            let validation_schema = self
                                .raw_request_body_schema(raw_operation.as_ref(), content_type)
                                .unwrap_or(
                                    serde_json::to_value(schema)
                                        .map_err(GeneratorError::ParseError)?,
                                );
                            Some(
                                self.resolve_or_inline_schema(schema, operation_id, "Request")
                                    .map(|schema_name| RequestBodyContent::Multipart {
                                        schema_name,
                                        media_type: content_type.to_string(),
                                        validation_schema,
                                    })?,
                            )
                        }
                        None => Some(RequestBodyContent::SchemaLess {
                            media_type: content_type.to_string(),
                        }),
                    }
                } else if is_binary_media_type(content_type, maybe_schema) {
                    if media_type_essence(content_type)
                        .eq_ignore_ascii_case("application/octet-stream")
                    {
                        Some(RequestBodyContent::OctetStream {
                            media_type: content_type.to_string(),
                        })
                    } else {
                        Some(RequestBodyContent::Binary {
                            media_type: content_type.to_string(),
                        })
                    }
                } else if crate::openapi::is_text_media_type(content_type) {
                    // Any character-data media type (text/plain, text/xml,
                    // application/xml, +xml suffixed) is buffered and handed
                    // to the handler as a lossless UTF-8 String; the server
                    // never parses the payload.
                    Some(RequestBodyContent::TextPlain {
                        media_type: content_type.to_string(),
                    })
                } else {
                    None
                };
            }
            if op_info.request_body.is_none() {
                let mut media_types = request_body
                    .content
                    .as_ref()
                    .map(|content| content.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                media_types.sort();
                if !media_types.is_empty() {
                    op_info.request_body = Some(RequestBodyContent::Unsupported { media_types });
                }
            }
        }

        // Extract response schemas
        if let Some(responses) = &operation.responses {
            for (status_code, response) in responses {
                let response = self.resolve_response(response)?;
                // T15: SSE auto-detection. If any response declares
                // `text/event-stream`, mark the operation as streaming. The
                // user can still override via config; here we lift the spec
                // signal so a `stream: true` parameter and an event-stream
                // content type produce a streaming variant by default.
                let supports_streaming = response.content.as_ref().is_some_and(|content| {
                    content
                        .keys()
                        .any(|ct| crate::openapi::is_event_stream_media_type(ct))
                });
                if supports_streaming {
                    op_info.supports_streaming = true;
                }

                let mut response_info = OperationResponse {
                    supports_streaming,
                    has_content: response
                        .content
                        .as_ref()
                        .is_some_and(|content| !content.is_empty()),
                    ..Default::default()
                };
                if let Some((media_type, schema)) = response.json_content() {
                    if let Some(schema_ref) = schema.reference() {
                        // Named schema reference
                        if let Some(schema_name) = self.extract_schema_name(schema_ref) {
                            op_info
                                .response_schemas
                                .insert(status_code.clone(), schema_name.to_string());
                            response_info.schema_name = Some(schema_name.to_string());
                            response_info.media_type = Some(media_type.to_string());
                            response_info.body = Some(OperationResponseBody::Json {
                                schema_name: schema_name.to_string(),
                                media_type: media_type.to_string(),
                            });
                        }
                    } else {
                        // Inline schema - generate a synthetic type name and analyze it
                        let synthetic_name =
                            self.generate_inline_response_type_name(operation_id, status_code);

                        // Use the existing inline schema infrastructure
                        let mut deps = HashSet::new();
                        let synthetic_name =
                            self.add_inline_schema(&synthetic_name, schema, &mut deps)?;

                        op_info
                            .response_schemas
                            .insert(status_code.clone(), synthetic_name.clone());
                        response_info.body = Some(OperationResponseBody::Json {
                            schema_name: synthetic_name.clone(),
                            media_type: media_type.to_string(),
                        });
                        response_info.schema_name = Some(synthetic_name);
                        response_info.media_type = Some(media_type.to_string());
                    }
                }
                if response_info.body.is_none()
                    && let Some(content) = response.content.as_ref()
                {
                    let selected = content
                        .iter()
                        .find(|(media_type, media)| {
                            matches!(
                                crate::openapi::classify_response_media_type(
                                    media_type,
                                    media.schema.as_ref()
                                ),
                                crate::openapi::ResponseMediaKind::Text
                            )
                        })
                        .or_else(|| {
                            content.iter().find(|(media_type, media)| {
                                matches!(
                                    crate::openapi::classify_response_media_type(
                                        media_type,
                                        media.schema.as_ref()
                                    ),
                                    crate::openapi::ResponseMediaKind::Binary
                                ) && !crate::openapi::is_wildcard_media_type(media_type)
                            })
                        })
                        .or_else(|| {
                            content.iter().find(|(media_type, media)| {
                                matches!(
                                    crate::openapi::classify_response_media_type(
                                        media_type,
                                        media.schema.as_ref()
                                    ),
                                    crate::openapi::ResponseMediaKind::Binary
                                )
                            })
                        });
                    if let Some((media_type, media)) = selected {
                        response_info.body = match crate::openapi::classify_response_media_type(
                            media_type,
                            media.schema.as_ref(),
                        ) {
                            crate::openapi::ResponseMediaKind::Text => {
                                Some(OperationResponseBody::Text {
                                    media_type: media_type.clone(),
                                })
                            }
                            crate::openapi::ResponseMediaKind::Binary => {
                                Some(OperationResponseBody::Binary {
                                    media_type: media_type.clone(),
                                    wildcard: crate::openapi::is_wildcard_media_type(media_type),
                                })
                            }
                            _ => None,
                        };
                    }
                }
                response_info.unsupported_media_types = response
                    .content
                    .as_ref()
                    .into_iter()
                    .flat_map(|content| content.iter())
                    .filter(|(media_type, content)| {
                        match crate::openapi::classify_response_media_type(
                            media_type,
                            content.schema.as_ref(),
                        ) {
                            crate::openapi::ResponseMediaKind::Json => content.schema.is_none(),
                            crate::openapi::ResponseMediaKind::Unsupported => true,
                            crate::openapi::ResponseMediaKind::EventStream
                            | crate::openapi::ResponseMediaKind::Text
                            | crate::openapi::ResponseMediaKind::Binary => false,
                        }
                    })
                    .map(|(media_type, _)| media_type.clone())
                    .collect();
                operation_responses.insert(status_code.clone(), response_info);
            }
        }

        // T15: detect a `stream` boolean parameter on the operation; pair it
        // with the SSE response signal above to populate stream_parameter.
        if op_info.supports_streaming
            && let Some(parameters) = &operation.parameters
        {
            for param in parameters {
                if let Some(name) = param.name.as_deref() {
                    if name.eq_ignore_ascii_case("stream") {
                        op_info.stream_parameter = Some(name.to_string());
                        break;
                    }
                }
            }
        }

        // Extract parameters (operation-level first, then merge path-item-level)
        if let Some(parameters) = &operation.parameters {
            for (index, param) in parameters.iter().enumerate() {
                // into_owned: analyze_parameter needs `&mut self` (it may
                // register an inline object schema for form-exploded query
                // params), which can't coexist with the Cow's `&self` borrow.
                let resolved = self.resolve_parameter(param).into_owned();
                let validation_schema = raw_operation
                    .as_ref()
                    .and_then(|operation| operation.get("parameters"))
                    .and_then(Value::as_array)
                    .and_then(|parameters| parameters.get(index))
                    .and_then(|parameter| self.raw_parameter_schema(parameter));
                if let Some(param_info) =
                    self.analyze_parameter(&resolved, operation_id, validation_schema)?
                {
                    op_info.parameters.push(param_info);
                }
            }
        }

        // Merge path-item-level parameters (operation params take precedence per OpenAPI spec)
        if let Some(path_params) = path_item_parameters {
            let existing_keys: std::collections::HashSet<(String, String)> = op_info
                .parameters
                .iter()
                .map(|p| (p.name.clone(), p.location.clone()))
                .collect();
            for (index, param) in path_params.iter().enumerate() {
                let resolved = self.resolve_parameter(param).into_owned();
                let validation_schema = raw_path_item
                    .as_ref()
                    .and_then(|path_item| path_item.get("parameters"))
                    .and_then(Value::as_array)
                    .and_then(|parameters| parameters.get(index))
                    .and_then(|parameter| self.raw_parameter_schema(parameter));
                if let Some(param_info) =
                    self.analyze_parameter(&resolved, operation_id, validation_schema)?
                {
                    if !existing_keys
                        .contains(&(param_info.name.clone(), param_info.location.clone()))
                    {
                        op_info.parameters.push(param_info);
                    }
                }
            }
        }

        // Synthesize path parameters that are referenced via `{var}` in the
        // path template but not declared as parameters in the spec.
        // langsmith/knocklabs/cloudflare hit this — `/repos/{owner}/{repo}/...`
        // declares `repo` but not `owner`. Without this, codegen emits
        // `format!("/repos/{owner}/...", repo)` and `owner` is undefined
        // (E0425). We synthesize each missing variable as a required
        // `String` path parameter.
        let mut declared_path_names: std::collections::HashSet<String> = op_info
            .parameters
            .iter()
            .filter(|p| p.location == "path")
            .map(|p| p.name.clone())
            .collect();
        let bytes = path.as_bytes().iter();
        let mut current = String::new();
        let mut in_brace = false;
        let mut synthesized: Vec<String> = Vec::new();
        for b in bytes {
            match *b {
                b'{' => {
                    in_brace = true;
                    current.clear();
                }
                b'}' if in_brace => {
                    in_brace = false;
                    if !current.is_empty() && !declared_path_names.contains(&current) {
                        synthesized.push(current.clone());
                        declared_path_names.insert(current.clone());
                    }
                }
                _ if in_brace => current.push(*b as char),
                _ => {}
            }
        }
        for name in synthesized {
            eprintln!(
                "⚠️  path `{}` references `{{{}}}` but the spec doesn't declare it as a parameter — synthesizing as required String",
                path, name
            );
            op_info.parameters.push(ParameterInfo {
                name,
                location: "path".to_string(),
                required: true,
                schema_ref: None,
                rust_type: "String".to_string(),
                description: None,
                enum_values: None,
                enum_varnames: None,
                rust_ident: None,
                query_serialization: None,
                validation_schema: None,
            });
        }

        // Disambiguate Rust idents across the operation. Real-world specs
        // sometimes use both `kebab-case` and `snake_case` for closely-related
        // filter parameters (vercel: `exclude_ids` + `exclude-ids`), or
        // operator-suffixed forms (twilio: `StartTime`, `StartTime<`,
        // `StartTime>`). Without disambiguation those parameters share a
        // single binding and the generated body fails E0382 (use of moved
        // value) or E0415 (binding declared twice).
        let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
        for p in op_info.parameters.iter_mut() {
            let raw = base_param_ident(&p.name);
            let mut chosen = raw.clone();
            let mut suffix = 2;
            while !used.insert(chosen.clone()) {
                chosen = format!("{raw}_{suffix}");
                suffix += 1;
            }
            p.rust_ident = Some(chosen);
        }

        Ok((op_info, operation_responses))
    }

    /// Resolve a local reusable Request Body Object through its JSON Pointer.
    fn resolve_request_body(
        &self,
        request_body: &crate::openapi::RequestBody,
    ) -> Result<crate::openapi::RequestBody> {
        let mut current = request_body.clone();
        let mut visited = HashSet::new();
        while let Some(reference) = current.reference.clone() {
            if !visited.insert(reference.clone()) {
                return Err(GeneratorError::CircularDependency(format!(
                    "request body reference {reference}"
                )));
            }

            let pointer = reference.strip_prefix('#').ok_or_else(|| {
                GeneratorError::UnresolvedReference(format!(
                    "external request body reference `{reference}` is not supported"
                ))
            })?;
            if !pointer.is_empty() && !pointer.starts_with('/') {
                return Err(GeneratorError::UnresolvedReference(format!(
                    "request body reference `{reference}` is not a local JSON Pointer"
                )));
            }
            let value = self.openapi_spec.pointer(pointer).ok_or_else(|| {
                GeneratorError::UnresolvedReference(format!(
                    "request body reference `{reference}` does not exist"
                ))
            })?;
            let object = value.as_object().ok_or_else(|| {
                GeneratorError::InvalidSchema(format!(
                    "request body reference `{reference}` must target an object"
                ))
            })?;
            if !["$ref", "description", "required", "content"]
                .iter()
                .any(|field| object.contains_key(*field))
            {
                return Err(GeneratorError::InvalidSchema(format!(
                    "request body reference `{reference}` does not target a structurally compatible OpenAPI Request Body Object"
                )));
            }
            current = serde_json::from_value(value.clone()).map_err(|error| {
                GeneratorError::InvalidSchema(format!(
                    "request body reference `{reference}` is not a valid OpenAPI Request Body Object: {error}"
                ))
            })?;
        }
        Ok(current)
    }

    /// Resolve a local reusable Response Object through its JSON Pointer.
    ///
    /// Real-world documents occasionally store a structurally valid Response
    /// Object under the wrong Components map. Resolving the pointer itself
    /// preserves compatibility with those documents while still validating
    /// that the target can be interpreted as a Response Object.
    fn resolve_response(
        &self,
        response: &crate::openapi::Response,
    ) -> Result<crate::openapi::Response> {
        let mut current = response.clone();
        let mut visited = HashSet::new();
        while let Some(reference) = current.reference.clone() {
            if !visited.insert(reference.clone()) {
                return Err(GeneratorError::CircularDependency(format!(
                    "response reference {reference}"
                )));
            }

            let pointer = reference.strip_prefix('#').ok_or_else(|| {
                GeneratorError::UnresolvedReference(format!(
                    "external response reference `{reference}` is not supported"
                ))
            })?;
            if !pointer.is_empty() && !pointer.starts_with('/') {
                return Err(GeneratorError::UnresolvedReference(format!(
                    "response reference `{reference}` is not a local JSON Pointer"
                )));
            }
            let value = self.openapi_spec.pointer(pointer).ok_or_else(|| {
                GeneratorError::UnresolvedReference(format!(
                    "response reference `{reference}` does not exist"
                ))
            })?;
            let object = value.as_object().ok_or_else(|| {
                GeneratorError::InvalidSchema(format!(
                    "response reference `{reference}` must target an object"
                ))
            })?;
            if !["$ref", "description", "headers", "content", "links"]
                .iter()
                .any(|field| object.contains_key(*field))
            {
                return Err(GeneratorError::InvalidSchema(format!(
                    "response reference `{reference}` does not target a structurally compatible OpenAPI Response Object"
                )));
            }
            current = serde_json::from_value(value.clone()).map_err(|error| {
                GeneratorError::InvalidSchema(format!(
                    "response reference `{reference}` is not a valid OpenAPI Response Object: {error}"
                ))
            })?;
        }
        Ok(current)
    }

    /// Generate a type name for an inline response schema.
    ///
    /// 200 (the canonical success status) keeps the unsuffixed `{Op}Response`
    /// name so simple specs and existing snapshots are unchanged. Every other
    /// status code is disambiguated by suffix so that multi-response operations
    /// (e.g. 200 + 400) don't collide in the schema registry — see issue #8.
    fn generate_inline_response_type_name(&self, operation_id: &str, status_code: &str) -> String {
        use heck::ToPascalCase;
        let base_name = operation_id.replace('.', "_").to_pascal_case();
        let suffix = Self::status_code_suffix(status_code);
        format!("{}Response{}", base_name, suffix)
    }

    /// Map an OpenAPI status code key to a suffix for generated type names.
    ///
    /// "200" → "" (unchanged, the dominant case)
    /// "201", "400", "404" → "201", "400", "404"
    /// "default" → "Default"
    /// "4XX" / "4xx" → "4xx" (lowercased range form)
    fn status_code_suffix(status_code: &str) -> String {
        match status_code {
            "" | "200" => String::new(),
            "default" | "Default" => "Default".to_string(),
            other if other.chars().all(|c| c.is_ascii_digit()) => other.to_string(),
            other => other.to_ascii_lowercase(),
        }
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
        self.add_inline_schema(&synthetic_name, schema, &mut deps)
    }

    /// Resolve a parameter reference ($ref) to the actual parameter definition.
    /// Returns the resolved parameter, or the original if it's not a reference.
    fn resolve_parameter<'a>(
        &'a self,
        param: &'a crate::openapi::Parameter,
    ) -> std::borrow::Cow<'a, crate::openapi::Parameter> {
        if let Some(ref_str) = param.reference.as_deref() {
            if let Some(param_name) = ref_str.strip_prefix("#/components/parameters/") {
                if let Some(resolved) = self.component_parameters.get(param_name) {
                    return std::borrow::Cow::Borrowed(resolved);
                }
            }
        }
        std::borrow::Cow::Borrowed(param)
    }

    /// Analyze a parameter.
    ///
    /// `operation_id` is used to generate a unique synthetic enum type name
    /// when the parameter's inline schema is a string with `enum` or `const`
    /// (e.g. `GetItemTheConstant`). The client generator emits the enum
    /// alongside the operation methods. See issue #10 follow-up.
    /// Look up `#/components/schemas/{name}` in the raw OpenAPI document and
    /// decide whether it's a string with enum values. Used by analyze_parameter
    /// (T10). String-enum refs flow through to the codegen-typed parameter
    /// path; object refs are typed only when form-exploded (issue #27), and
    /// other struct refs stay `String` until deepObject / explode=false
    /// serialization is generated (T14).
    fn referenced_schema_is_string_enum(&self, name: &str) -> bool {
        if self.resolve_cached_schema(name).is_some_and(|schema| {
            matches!(
                schema.schema_type,
                SchemaType::StringEnum { .. } | SchemaType::ExtensibleEnum { .. }
            )
        }) {
            return true;
        }
        let Some(schema_value) = self
            .openapi_spec
            .get("components")
            .and_then(|c| c.get("schemas"))
            .and_then(|s| s.get(name))
        else {
            return false;
        };
        let is_string_type = schema_value
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| s == "string")
            .unwrap_or(false);
        let has_enum_or_const =
            schema_value.get("enum").is_some() || schema_value.get("const").is_some();
        is_string_type && has_enum_or_const
    }

    fn resolve_raw_local_reference(&self, value: &Value) -> Option<Value> {
        let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
            return Some(value.clone());
        };
        let pointer = reference.strip_prefix('#')?;
        self.openapi_spec.pointer(pointer).cloned()
    }

    fn raw_request_body_schema(
        &self,
        operation: Option<&Value>,
        content_type: &str,
    ) -> Option<Value> {
        let request_body = operation?.get("requestBody")?;
        self.resolve_raw_local_reference(request_body)?
            .get("content")?
            .get(content_type)?
            .get("schema")
            .cloned()
    }

    fn raw_parameter_schema(&self, parameter: &Value) -> Option<Value> {
        self.resolve_raw_local_reference(parameter)?
            .get("schema")
            .cloned()
    }

    fn analyze_parameter(
        &mut self,
        param: &crate::openapi::Parameter,
        operation_id: &str,
        raw_validation_schema: Option<Value>,
    ) -> Result<Option<ParameterInfo>> {
        use heck::ToPascalCase;

        let name = param.name.as_deref().unwrap_or("");
        let location = param.location.as_deref().unwrap_or("");
        let required = param.required.unwrap_or(false);
        let validation_schema = match raw_validation_schema {
            Some(schema) => Some(schema),
            None => param
                .schema
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(GeneratorError::ParseError)?,
        };

        let mut rust_type = "String".to_string();
        let mut schema_ref = None;
        let mut enum_values: Option<Vec<String>> = None;
        let mut enum_varnames: Option<Vec<String>> = None;
        let mut query_serialization: Option<QuerySerialization> = None;

        // OAS 3.x style/explode resolution for `in: query`. Defaults are
        // style=form and — for form only — explode=true, so an object/array
        // query parameter with nothing specified is already form-exploded
        // per spec (issue #27). deepObject is only defined with explode=true;
        // an explicit explode=false there is undefined and keeps the fallback.
        let is_query = location == "query";
        let is_simple_header = location == "header"
            && matches!(param.style.as_deref(), None | Some("simple"))
            && param.explode != Some(true);
        let form_style = matches!(param.style.as_deref(), None | Some("form"));
        let form_exploded = form_style && param.explode.unwrap_or(true);
        let deep_object =
            param.style.as_deref() == Some("deepObject") && param.explode != Some(false);

        let object_serialization = if !is_query {
            None
        } else if deep_object {
            Some(QuerySerialization::DeepObject)
        } else if form_exploded {
            Some(QuerySerialization::FormExplodedObject)
        } else if form_style {
            Some(QuerySerialization::FormObject)
        } else {
            None
        };

        if let Some(schema) = &param.schema {
            if let Some(ref_str) = schema.reference() {
                // T10: keep the resolved type when the target is a string-enum
                // (then `Display`/`as_str` are emitted, see generate_string_enum).
                // Object refs on query params with a generated wire style keep
                // the resolved struct type too (T14/issue #27); anything else
                // stays on the opaque `String` fallback.
                if let Some(name) = self.extract_schema_name(ref_str) {
                    if self.referenced_schema_is_string_enum(name) {
                        schema_ref = Some(name.to_string());
                    } else if object_serialization.is_some()
                        && self.referenced_schema_is_object(name)
                    {
                        schema_ref = Some(name.to_string());
                        query_serialization = if form_exploded && self.uses_aws_query_conventions()
                        {
                            match self.referenced_array_struct_item_type(name, 1) {
                                Some(ArrayItemType::NestedStructRef { properties, .. }) => {
                                    Some(QuerySerialization::FormExplodedNestedObject {
                                        properties,
                                    })
                                }
                                _ => object_serialization.clone(),
                            }
                        } else {
                            object_serialization.clone()
                        };
                    } else if (is_query && form_style || is_simple_header)
                        && let Some(item_type) = self.referenced_array_param_item_type(name)
                    {
                        // A parameter may reference a reusable array schema
                        // rather than declaring `type: array` inline. Preserve
                        // that component as a pruning root while projecting the
                        // public parameter type to the same Vec<T> used by
                        // inline arrays.
                        schema_ref = Some(name.to_string());
                        query_serialization = Some(if is_simple_header {
                            QuerySerialization::SimpleHeaderArray { item_type }
                        } else if form_exploded {
                            QuerySerialization::FormExplodedArray { item_type }
                        } else {
                            QuerySerialization::FormArray { item_type }
                        });
                    }
                }
            } else if object_serialization.is_some() && Self::schema_is_inline_object(schema) {
                // Inline object schema on a query parameter with a generated
                // wire style: synthesize a struct (e.g. `FindWidgetsFilter`)
                // so the caller passes typed fields instead of a pre-encoded
                // string.
                let op_pascal = operation_id.replace('.', "_").to_pascal_case();
                let param_pascal = name.to_pascal_case();
                let synthetic_name = format!("{op_pascal}{param_pascal}");
                let mut deps = HashSet::new();
                let synthetic_name = self.add_inline_schema(&synthetic_name, schema, &mut deps)?;
                schema_ref = Some(synthetic_name.clone());
                query_serialization = if form_exploded && self.uses_aws_query_conventions() {
                    match self.referenced_array_struct_item_type(&synthetic_name, 1) {
                        Some(ArrayItemType::NestedStructRef { properties, .. }) => {
                            Some(QuerySerialization::FormExplodedNestedObject { properties })
                        }
                        _ => object_serialization.clone(),
                    }
                } else {
                    object_serialization.clone()
                };
            } else if (is_query && form_style || is_simple_header)
                && matches!(
                    schema.schema_type(),
                    Some(crate::openapi::SchemaType::Array)
                )
                && let Some(item_type) = self.array_param_item_type(schema)
            {
                // Typed form-style array (openapi-generator-anu): the client
                // takes `Vec<item_type>` and emits repeated (explode=true) or
                // comma-joined (explode=false) pairs. `rust_type` deliberately
                // stays "String" because the shared query-serialization plan
                // is the authoritative Vec<T> projection. Arrays whose items
                // don't type (objects, nested arrays) fall through to the
                // explicit unsupported shape below.
                query_serialization = Some(if is_simple_header {
                    QuerySerialization::SimpleHeaderArray { item_type }
                } else if form_exploded {
                    QuerySerialization::FormExplodedArray { item_type }
                } else {
                    QuerySerialization::FormArray { item_type }
                });
            } else if let Some(schema_type) = schema.schema_type() {
                // Route integer/number through the same TypeMapper the schema
                // property path uses (see analyze_property), so `format: int32`
                // yields `i32` and `[type_mappings]`/strategy config applies to
                // parameters too. Hardcoding `i64`/`f64` here previously made
                // `format` and config impossible to honour for query/path params.
                let format = schema.details().format.clone();
                rust_type = match schema_type {
                    crate::openapi::SchemaType::Boolean => "bool".to_string(),
                    crate::openapi::SchemaType::Integer => self.integer_rust_type(schema.details()),
                    crate::openapi::SchemaType::Number => {
                        self.type_mapper.number_format(format.as_deref()).rust_type
                    }
                    crate::openapi::SchemaType::String => "String".to_string(),
                    _ => "String".to_string(),
                };

                if matches!(schema_type, crate::openapi::SchemaType::String) {
                    let details = schema.details();
                    if details.is_string_enum() {
                        if let Some(values) = details.string_enum_values() {
                            if !values.is_empty() {
                                let op_pascal = operation_id.replace('.', "_").to_pascal_case();
                                let param_pascal = name.to_pascal_case();
                                rust_type = format!("{op_pascal}{param_pascal}");
                                // Honor `x-enum-varnames` here the same way
                                // schema-level enums do. A mismatched length is
                                // ambiguous about which value each name refers
                                // to, so drop it rather than guess.
                                enum_varnames = details
                                    .extra
                                    .get("x-enum-varnames")
                                    .and_then(Value::as_array)
                                    .map(|raw| {
                                        raw.iter()
                                            .filter_map(Value::as_str)
                                            .map(str::to_owned)
                                            .collect::<Vec<_>>()
                                    })
                                    .filter(|names| names.len() == values.len());
                                enum_values = Some(values);
                            }
                        }
                    }
                }
            }

            if is_query && query_serialization.is_none() {
                let referenced_name = schema
                    .reference()
                    .and_then(|reference| self.extract_schema_name(reference));
                let is_object = referenced_name
                    .is_some_and(|name| self.referenced_schema_is_object(name))
                    || Self::schema_is_inline_object(schema);
                let is_array = referenced_name
                    .is_some_and(|name| self.referenced_schema_is_array(name))
                    || matches!(
                        schema.schema_type(),
                        Some(crate::openapi::SchemaType::Array)
                    );
                let is_composed = referenced_name
                    .is_some_and(|name| self.referenced_schema_is_composed_query_shape(name));
                let reason = if param.style.as_deref() == Some("deepObject")
                    && param.explode == Some(false)
                {
                    Some("style=deepObject with explode=false is undefined by OpenAPI".to_string())
                } else if param.style.as_deref() == Some("deepObject") && !is_object {
                    Some("style=deepObject is defined only for object query parameters".to_string())
                } else if is_object {
                    Some(format!(
                        "object query parameters do not support style={}",
                        param.style.as_deref().unwrap_or("form")
                    ))
                } else if is_array && form_style {
                    Some(
                        "form array query parameter exceeds the supported nesting bound or contains a non-scalar leaf; supported shapes are scalar arrays, arrays of flat scalar objects, and one nested scalar-object array"
                            .to_string(),
                    )
                } else if is_array {
                    Some(format!(
                        "array query parameters do not yet support style={}",
                        param.style.as_deref().unwrap_or("form")
                    ))
                } else if is_composed {
                    Some(
                        "composed or union query schemas cannot be projected to an unambiguous flat wire shape"
                            .to_string(),
                    )
                } else {
                    None
                };
                if let Some(reason) = reason {
                    query_serialization = Some(QuerySerialization::Unsupported { reason });
                }
            }
        }

        Ok(Some(ParameterInfo {
            name: name.to_string(),
            location: location.to_string(),
            required,
            schema_ref,
            rust_type,
            description: param.description.clone(),
            enum_values,
            enum_varnames,
            rust_ident: None,
            query_serialization,
            validation_schema,
        }))
    }

    /// Rust item type for a typed array query parameter
    /// (openapi-generator-anu). Scalar items map through the TypeMapper;
    /// $ref items resolve when the target is a scalar alias or generated
    /// string enum (both support the client/server string wire projection).
    /// Anything else — objects, nested arrays — returns None and the
    /// parameter keeps the opaque-string fallback. Inline-enum'd string
    /// items stay plain `String`: the op-scoped enum synthesis (issue #10)
    /// is wired for scalar params only.
    fn array_param_item_type(&self, schema: &crate::openapi::Schema) -> Option<ArrayItemType> {
        let items = schema.details().item_schema()?;
        // AWS query-protocol specs wrap item refs in an annotation-only allOf
        // (`items: {allOf: [$ref, {xml: ...}]}`). See through the wrapper when
        // every sibling is annotation-only, mirroring the type-alias rule.
        let unwrapped = unwrap_annotation_allof(items);
        if let Some(ref_str) = unwrapped.reference() {
            let name = self.extract_schema_name(ref_str)?;
            return self
                .referenced_array_scalar_item_type(name)
                .or_else(|| self.referenced_array_struct_item_type(name, 1));
        }
        let format = unwrapped.details().format.clone();
        let scalar = match unwrapped.schema_type()? {
            crate::openapi::SchemaType::String => "String".to_string(),
            crate::openapi::SchemaType::Integer => self.integer_rust_type(unwrapped.details()),
            crate::openapi::SchemaType::Number => {
                self.type_mapper.number_format(format.as_deref()).rust_type
            }
            crate::openapi::SchemaType::Boolean => "bool".to_string(),
            _ => return None,
        };
        Some(ArrayItemType::Scalar(scalar))
    }

    /// Resolve a reusable component array (including `$ref` aliases) and
    /// apply the same item projection as an inline array parameter.
    fn referenced_array_param_item_type(&self, name: &str) -> Option<ArrayItemType> {
        let schema = self.resolve_cached_schema(name)?;
        let SchemaType::Array { item_type } = &schema.schema_type else {
            return None;
        };
        self.analyzed_array_item_type(item_type)
    }

    fn analyzed_array_item_type(&self, item_type: &SchemaType) -> Option<ArrayItemType> {
        self.analyzed_array_item_type_at_depth(item_type, 1)
    }

    /// Accept a referenced structure as a form-style array item when every
    /// property is scalar (AWS query-protocol flat structures such as
    /// `Tag { Key, Value }`). Nested objects, arrays, and maps are rejected
    /// because the wire shape below one level is service-specific.
    fn referenced_array_struct_item_type(
        &self,
        name: &str,
        nested_array_depth: usize,
    ) -> Option<ArrayItemType> {
        let resolved = self.resolve_cached_schema(name)?;
        let SchemaType::Object {
            properties,
            required,
            additional_properties,
            ..
        } = &resolved.schema_type
        else {
            return None;
        };
        if properties.is_empty()
            || !matches!(additional_properties, ObjectAdditionalProperties::Forbidden)
        {
            return None;
        }
        let mut projected = Vec::with_capacity(properties.len());
        let mut has_array = false;
        for (wire_name, property) in properties {
            let value_type = if let Some(scalar) = self.query_scalar_type(&property.schema_type) {
                QueryStructPropertyType::Scalar(scalar)
            } else {
                if nested_array_depth == 0 {
                    return None;
                }
                if let Some(array) = self.resolve_query_array_type(&property.schema_type) {
                    let item_type =
                        self.analyzed_array_item_type_at_depth(array, nested_array_depth - 1)?;
                    if matches!(item_type, ArrayItemType::NestedStructRef { .. }) {
                        return None;
                    }
                    has_array = true;
                    QueryStructPropertyType::Array { item_type }
                } else {
                    has_array = true;
                    QueryStructPropertyType::Object {
                        properties: self.query_flat_object_properties(&property.schema_type)?,
                    }
                }
            };
            projected.push(QueryStructProperty {
                wire_name: wire_name.clone(),
                required: required.contains(wire_name),
                value_type,
            });
        }
        if has_array {
            Some(ArrayItemType::NestedStructRef {
                schema_name: name.to_string(),
                properties: projected,
            })
        } else {
            Some(ArrayItemType::FlatStructRef {
                schema_name: name.to_string(),
                properties: projected,
            })
        }
    }

    fn analyzed_array_item_type_at_depth(
        &self,
        item_type: &SchemaType,
        nested_array_depth: usize,
    ) -> Option<ArrayItemType> {
        match item_type {
            SchemaType::Primitive { rust_type, .. } => {
                Some(ArrayItemType::Scalar(rust_type.clone()))
            }
            SchemaType::Reference { target } => self
                .referenced_array_scalar_item_type(target)
                .or_else(|| self.referenced_array_struct_item_type(target, nested_array_depth)),
            _ => None,
        }
    }

    fn resolve_query_array_type<'a>(
        &'a self,
        schema_type: &'a SchemaType,
    ) -> Option<&'a SchemaType> {
        match schema_type {
            SchemaType::Array { item_type } => Some(item_type),
            SchemaType::Reference { target } => {
                let resolved = self.resolve_cached_schema(target)?;
                let SchemaType::Array { item_type } = &resolved.schema_type else {
                    return None;
                };
                Some(item_type)
            }
            _ => None,
        }
    }

    fn query_flat_object_properties(
        &self,
        schema_type: &SchemaType,
    ) -> Option<Vec<QueryStructProperty>> {
        let schema_type = match schema_type {
            SchemaType::Reference { target } => &self.resolve_cached_schema(target)?.schema_type,
            other => other,
        };
        let SchemaType::Object {
            properties,
            required,
            additional_properties,
            ..
        } = schema_type
        else {
            return None;
        };
        if properties.is_empty()
            || !matches!(additional_properties, ObjectAdditionalProperties::Forbidden)
        {
            return None;
        }
        properties
            .iter()
            .map(|(wire_name, property)| {
                Some(QueryStructProperty {
                    wire_name: wire_name.clone(),
                    required: required.contains(wire_name),
                    value_type: QueryStructPropertyType::Scalar(
                        self.query_scalar_type(&property.schema_type)?,
                    ),
                })
            })
            .collect()
    }

    fn query_scalar_type(&self, schema_type: &SchemaType) -> Option<QueryScalarType> {
        match schema_type {
            SchemaType::Primitive { rust_type, .. } => match rust_type.as_str() {
                "String" => Some(QueryScalarType::String),
                "bool" => Some(QueryScalarType::Boolean),
                value if value.starts_with('i') || value.starts_with('u') => {
                    Some(QueryScalarType::Integer)
                }
                value if value.starts_with('f') => Some(QueryScalarType::Number),
                "serde_json::Value" => None,
                _ => Some(QueryScalarType::String),
            },
            SchemaType::StringEnum { .. } | SchemaType::ExtensibleEnum { .. } => {
                Some(QueryScalarType::String)
            }
            SchemaType::Reference { target } => {
                let resolved = self.resolve_cached_schema(target)?;
                self.query_scalar_type(&resolved.schema_type)
            }
            _ => None,
        }
    }

    /// Resolve a referenced array item through any alias chain while
    /// preserving the outer schema name used by the public `Vec<T>` type.
    ///
    /// `SchemaType::Primitive` also represents dynamic JSON/object fallbacks,
    /// so require an actual OpenAPI scalar `type` before accepting it as a
    /// form-style query item. Unresolved and cyclic chains are rejected by
    /// `resolve_cached_schema`.
    fn referenced_array_scalar_item_type(&self, name: &str) -> Option<ArrayItemType> {
        let resolved = self.resolve_cached_schema(name)?;
        let supported = match &resolved.schema_type {
            SchemaType::StringEnum { .. } | SchemaType::ExtensibleEnum { .. } => true,
            SchemaType::Primitive { .. } => resolved
                .original
                .get("type")
                .is_some_and(Self::query_scalar_type_value),
            _ => false,
        };
        supported.then(|| ArrayItemType::SchemaRef(name.to_string()))
    }

    fn query_scalar_type_value(value: &Value) -> bool {
        const SCALARS: [&str; 4] = ["string", "integer", "number", "boolean"];
        if let Some(value) = value.as_str() {
            return SCALARS.contains(&value);
        }
        let Some(values) = value.as_array() else {
            return false;
        };
        if !values.iter().all(Value::is_string) {
            return false;
        }
        let mut non_null = values
            .iter()
            .filter_map(Value::as_str)
            .filter(|value| *value != "null");
        let Some(scalar) = non_null.next() else {
            return false;
        };
        non_null.next().is_none() && SCALARS.contains(&scalar)
    }

    /// True when a component (following `$ref` aliases) analyzes to an object.
    /// Used to decide whether a referenced query parameter can use a typed
    /// object serialization plan (issue #27).
    fn referenced_schema_is_object(&self, name: &str) -> bool {
        self.resolve_cached_schema(name)
            .is_some_and(|schema| matches!(schema.schema_type, SchemaType::Object { .. }))
    }

    fn referenced_schema_is_array(&self, name: &str) -> bool {
        self.resolve_cached_schema(name)
            .is_some_and(|schema| matches!(schema.schema_type, SchemaType::Array { .. }))
    }

    fn referenced_schema_is_composed_query_shape(&self, name: &str) -> bool {
        self.resolve_cached_schema(name).is_some_and(|schema| {
            matches!(
                schema.schema_type,
                SchemaType::Composition { .. }
                    | SchemaType::Union { .. }
                    | SchemaType::DiscriminatedUnion { .. }
            )
        })
    }

    fn resolve_cached_schema(&self, name: &str) -> Option<&AnalyzedSchema> {
        let mut current = name;
        let mut visited = HashSet::new();
        loop {
            if !visited.insert(current) {
                return None;
            }
            let schema = self.resolved_cache.get(current)?;
            if let SchemaType::Reference { target } = &schema.schema_type {
                current = target;
            } else {
                return Some(schema);
            }
        }
    }

    /// Inline-schema counterpart of [`Self::referenced_schema_is_object`].
    fn schema_is_inline_object(schema: &crate::openapi::Schema) -> bool {
        match schema.schema_type() {
            Some(crate::openapi::SchemaType::Object) => true,
            None => schema.details().properties.is_some(),
            _ => false,
        }
    }
}

/// Deserialize the whole document, locating any failure to the node that
/// caused it.
///
/// `serde_json::from_value` reports only serde's innermost message — for the
/// untagged `Schema` enum that is "data did not match any variant of untagged
/// enum Schema", with no field, schema name, or position. Tracking the path
/// turns that into a JSON Pointer the author can jump straight to (issue #60).
/// A Rust type name for a JSON Pointer target, e.g.
/// `/components/schemas/Tag/allOf/0` becomes `TagAllOf0`. The `components`
/// section prefix carries no information and is dropped.
fn pointer_type_name(pointer: &str) -> String {
    use heck::ToPascalCase;

    pointer
        .split('/')
        .skip(1)
        .filter(|segment| !matches!(*segment, "components" | "schemas" | "properties"))
        .map(|segment| {
            segment
                .replace("~1", "/")
                .replace("~0", "~")
                .to_pascal_case()
        })
        .collect::<String>()
}

/// The one schema every position shares, if they are interchangeable: the same
/// `$ref`, or the same primitive type and format. Inline objects never qualify
/// — two structurally identical inline objects still hoist two named types, so
/// treating them as one element type would silently drop a generated name.
fn shared_positional_schema(positions: &[Schema]) -> Option<&Schema> {
    let first = positions.first()?;
    let key = positional_schema_key(first)?;
    positions
        .iter()
        .skip(1)
        .all(|position| positional_schema_key(position).as_deref() == Some(key.as_str()))
        .then_some(first)
}

fn positional_schema_key(schema: &Schema) -> Option<String> {
    if let Some(reference) = schema.reference() {
        return Some(format!("$ref {reference}"));
    }
    let details = schema.details();
    if details.properties.is_some() || details.enum_values.is_some() || details.items.is_some() {
        return None;
    }
    match schema.schema_type()? {
        crate::openapi::SchemaType::Object | crate::openapi::SchemaType::Array => None,
        scalar => Some(format!(
            "{scalar:?} {}",
            details.format.as_deref().unwrap_or_default()
        )),
    }
}

fn parse_spec_document(openapi_spec: &Value) -> Result<OpenApiSpec> {
    serde_path_to_error::deserialize(openapi_spec).map_err(|error| {
        let mut pointer = json_pointer(error.path());
        // Untagged enums deserialize from a buffered copy, so serde's path
        // stops at the outermost `Schema` — usually the component schema.
        // Walk the failing subtree to name the node that actually failed.
        pointer.push_str(&refine_schema_failure(openapi_spec, &pointer));
        GeneratorError::ParseErrorAt {
            pointer,
            message: error.into_inner().to_string(),
        }
    })
}

/// Schema keywords holding a single subschema.
const SUBSCHEMA_KEYWORDS: [&str; 11] = [
    "items",
    "additionalProperties",
    "propertyNames",
    "unevaluatedProperties",
    "unevaluatedItems",
    "contains",
    "contentSchema",
    "if",
    "then",
    "else",
    "not",
];

/// Schema keywords holding a list of subschemas.
const SUBSCHEMA_LIST_KEYWORDS: [&str; 4] = ["oneOf", "anyOf", "allOf", "prefixItems"];

/// Schema keywords holding a map of named subschemas.
const SUBSCHEMA_MAP_KEYWORDS: [&str; 5] = [
    "properties",
    "patternProperties",
    "dependentSchemas",
    "$defs",
    "definitions",
];

/// Budget on parse attempts while refining a located failure. Refinement runs
/// only on the error path, but a multi-megabyte document should still not turn
/// one bad keyword into an unbounded search.
const REFINE_PARSE_BUDGET: usize = 20_000;

/// Extend a located parse failure with the pointer suffix of the malformed
/// schema below it, so the reported pointer names the offending keyword rather
/// than the enclosing component schema, path item, or `paths` map.
///
/// Serde's own path stops at the first `#[serde(flatten)]` or untagged enum it
/// buffers through — for a document that is `#/paths` or the component schema —
/// so the rest of the descent happens here.
fn refine_schema_failure(openapi_spec: &Value, pointer: &str) -> String {
    let Some(path) = pointer.strip_prefix('#') else {
        return String::new();
    };
    let Some(node) = openapi_spec.pointer(path) else {
        return String::new();
    };
    let segments = path.split('/').skip(1).collect::<Vec<_>>();
    let last = segments.last().copied().unwrap_or_default();
    let parent = segments
        .len()
        .checked_sub(2)
        .map(|index| segments[index])
        .unwrap_or_default();

    if last == "schema" || holds_schemas(parent) {
        return if parses_as_schema(node) {
            String::new()
        } else {
            deepest_schema_failure(node)
        };
    }

    let mut budget = REFINE_PARSE_BUDGET;
    locate_failing_schema(node, holds_schemas(last), &mut budget).unwrap_or_default()
}

/// Whether a key's members are schemas: the Components `schemas` map and the
/// JSON Schema `$defs` / `definitions` maps.
fn holds_schemas(key: &str) -> bool {
    matches!(key, "schemas" | "$defs" | "definitions")
}

/// Walk OpenAPI structure looking for the malformed schema, then drill into it
/// keyword-first.
///
/// Only nodes in a schema position are tested. Guessing from shape does not
/// work: a `properties` map whose single property is named `properties` is
/// indistinguishable from a schema by its keys alone, and fails to parse as one
/// — naming it would point the author at a node that is perfectly valid.
fn locate_failing_schema(
    node: &Value,
    children_are_schemas: bool,
    budget: &mut usize,
) -> Option<String> {
    for (segment, key, child) in child_nodes(node) {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        if children_are_schemas || key == "schema" {
            if !parses_as_schema(child) {
                return Some(format!("/{segment}{}", deepest_schema_failure(child)));
            }
            continue;
        }
        if let Some(rest) = locate_failing_schema(child, holds_schemas(key), budget) {
            return Some(format!("/{segment}{rest}"));
        }
    }
    None
}

fn parses_as_schema(node: &Value) -> bool {
    Schema::deserialize(node).is_ok()
}

/// Depth-first search inside a malformed schema for the deepest subschema that
/// also fails to parse, so the pointer names the offending keyword rather than
/// the schema that contains it. The caller guarantees `node` already failed.
fn deepest_schema_failure(node: &Value) -> String {
    let Some(object) = node.as_object() else {
        return String::new();
    };

    let descend = |segment: String, child: &Value| -> Option<String> {
        if parses_as_schema(child) {
            return None;
        }
        Some(format!("/{segment}{}", deepest_schema_failure(child)))
    };

    for keyword in SUBSCHEMA_KEYWORDS {
        if let Some(child) = object.get(keyword)
            && let Some(suffix) = descend(escape_pointer_segment(keyword), child)
        {
            return suffix;
        }
    }
    for keyword in SUBSCHEMA_LIST_KEYWORDS {
        if let Some(Value::Array(children)) = object.get(keyword) {
            for (index, child) in children.iter().enumerate() {
                if let Some(suffix) = descend(
                    format!("{}/{index}", escape_pointer_segment(keyword)),
                    child,
                ) {
                    return suffix;
                }
            }
        }
    }
    for keyword in SUBSCHEMA_MAP_KEYWORDS {
        if let Some(Value::Object(children)) = object.get(keyword) {
            for (name, child) in children {
                if let Some(suffix) = descend(
                    format!(
                        "{}/{}",
                        escape_pointer_segment(keyword),
                        escape_pointer_segment(name)
                    ),
                    child,
                ) {
                    return suffix;
                }
            }
        }
    }
    String::new()
}

/// Object members and array elements, paired with their JSON Pointer segment
/// and raw key. Scalars have no children and are skipped, as are members that
/// hold data rather than schemas: an `x-` extension or an `example` payload is
/// free-form JSON that never fails to deserialize, so anything schema-shaped
/// found in there is a coincidence, not the failure being located.
fn child_nodes(node: &Value) -> Vec<(String, &str, &Value)> {
    const DATA_KEYWORDS: [&str; 5] = ["example", "examples", "default", "enum", "const"];

    match node {
        Value::Object(members) => members
            .iter()
            .filter(|(name, child)| {
                (child.is_object() || child.is_array())
                    && !name.starts_with("x-")
                    && !DATA_KEYWORDS.contains(&name.as_str())
            })
            .map(|(name, child)| (escape_pointer_segment(name), name.as_str(), child))
            .collect(),
        Value::Array(elements) => elements
            .iter()
            .enumerate()
            .filter(|(_, child)| child.is_object() || child.is_array())
            .map(|(index, child)| (index.to_string(), "", child))
            .collect(),
        _ => Vec::new(),
    }
}

fn escape_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

/// Render a serde path as an RFC 6901 JSON Pointer (`#/components/schemas/Foo`)
/// so it can be pasted into any spec tooling. `~` and `/` inside a key are
/// escaped per the RFC.
fn json_pointer(path: &serde_path_to_error::Path) -> String {
    use serde_path_to_error::Segment;

    let mut pointer = String::from("#");
    for segment in path.iter() {
        match segment {
            Segment::Seq { index } => {
                pointer.push('/');
                pointer.push_str(&index.to_string());
            }
            Segment::Map { key } | Segment::Enum { variant: key } => {
                pointer.push('/');
                pointer.push_str(&escape_pointer_segment(key));
            }
            Segment::Unknown => pointer.push_str("/?"),
        }
    }
    pointer
}

pub(crate) fn component_schema_name_aliases(openapi_spec: &Value) -> BTreeMap<String, String> {
    let Some(schemas) = openapi_spec
        .pointer("/components/schemas")
        .and_then(Value::as_object)
    else {
        return BTreeMap::new();
    };

    let mut names_by_rust_name = BTreeMap::<String, Vec<String>>::new();
    for name in schemas.keys() {
        names_by_rust_name
            .entry(crate::generator::rust_type_name(name))
            .or_default()
            .push(name.clone());
    }

    // Reserve every identifier already represented by the document so a
    // suffix never steals another component's canonical Rust name.
    let mut claimed_rust_names = names_by_rust_name.keys().cloned().collect::<HashSet<_>>();
    let mut aliases = BTreeMap::new();

    for (rust_name, mut names) in names_by_rust_name {
        if names.len() < 2 {
            continue;
        }

        // Prefer an already-canonical component key (for example `Alert`
        // over `alert`), then use lexical order for deterministic results.
        names.sort_by_key(|name| (name != &rust_name, name.clone()));
        for source_name in names.into_iter().skip(1) {
            let mut suffix = 2;
            let replacement = loop {
                let candidate = format!("{rust_name}{suffix}");
                if claimed_rust_names.insert(candidate.clone()) {
                    break candidate;
                }
                suffix += 1;
            };

            aliases.insert(source_name, replacement);
        }
    }

    aliases
}

fn disambiguate_component_schema_names(openapi_spec: &mut Value) {
    let aliases = component_schema_name_aliases(openapi_spec);
    if aliases.is_empty() {
        return;
    }

    for (source_name, replacement) in &aliases {
        let rust_name = crate::generator::rust_type_name(source_name);
        eprintln!(
            "⚠️  schema `{source_name}` maps to the existing Rust type `{rust_name}` — disambiguated to `{replacement}`"
        );
    }

    let Some(schemas) = openapi_spec
        .pointer_mut("/components/schemas")
        .and_then(Value::as_object_mut)
    else {
        return;
    };

    let original_schemas = std::mem::take(schemas);
    for (name, schema) in original_schemas {
        schemas.insert(aliases.get(&name).cloned().unwrap_or(name), schema);
    }

    rewrite_component_schema_references(openapi_spec, &aliases);
}

fn disambiguate_analyzed_schema_names(
    analysis: &mut SchemaAnalysis,
    component_schemas: &BTreeMap<String, Schema>,
) {
    let mut names_by_rust_name = BTreeMap::<String, Vec<String>>::new();
    for name in analysis.schemas.keys() {
        names_by_rust_name
            .entry(crate::generator::rust_type_name(name))
            .or_default()
            .push(name.clone());
    }

    let mut claimed_rust_names = names_by_rust_name.keys().cloned().collect::<HashSet<_>>();
    let mut aliases = BTreeMap::<String, String>::new();

    for (rust_name, mut names) in names_by_rust_name {
        if names.len() < 2 {
            continue;
        }
        names.sort_by_key(|name| {
            (
                !component_schemas.contains_key(name),
                name != &rust_name,
                name.clone(),
            )
        });

        for source_name in names.into_iter().skip(1) {
            let mut suffix = 2;
            let replacement = loop {
                let candidate = format!("{rust_name}{suffix}");
                if claimed_rust_names.insert(candidate.clone()) {
                    break candidate;
                }
                suffix += 1;
            };
            eprintln!(
                "⚠️  generated schema `{source_name}` maps to the existing Rust type `{rust_name}` — disambiguated to `{replacement}`"
            );
            aliases.insert(source_name, replacement);
        }
    }

    if aliases.is_empty() {
        return;
    }

    let original_schemas = std::mem::take(&mut analysis.schemas);
    for (name, mut schema) in original_schemas {
        schema.name = renamed_schema_name(&schema.name, &aliases);
        schema.dependencies = schema
            .dependencies
            .into_iter()
            .map(|name| renamed_schema_name(&name, &aliases))
            .collect();
        rewrite_schema_type_names(&mut schema.schema_type, &aliases);
        analysis
            .schemas
            .insert(renamed_schema_name(&name, &aliases), schema);
    }

    let original_edges = std::mem::take(&mut analysis.dependencies.edges);
    for (name, dependencies) in original_edges {
        analysis.dependencies.edges.insert(
            renamed_schema_name(&name, &aliases),
            dependencies
                .into_iter()
                .map(|name| renamed_schema_name(&name, &aliases))
                .collect(),
        );
    }
    analysis.dependencies.recursive_schemas = analysis
        .dependencies
        .recursive_schemas
        .iter()
        .map(|name| renamed_schema_name(name, &aliases))
        .collect();

    analysis.patterns.tagged_enum_schemas = analysis
        .patterns
        .tagged_enum_schemas
        .iter()
        .map(|name| renamed_schema_name(name, &aliases))
        .collect();
    analysis.patterns.untagged_enum_schemas = analysis
        .patterns
        .untagged_enum_schemas
        .iter()
        .map(|name| renamed_schema_name(name, &aliases))
        .collect();
    analysis.patterns.type_mappings = std::mem::take(&mut analysis.patterns.type_mappings)
        .into_iter()
        .map(|(name, mappings)| {
            (
                renamed_schema_name(&name, &aliases),
                mappings
                    .into_iter()
                    .map(|(value, schema_name)| {
                        (value, renamed_schema_name(&schema_name, &aliases))
                    })
                    .collect(),
            )
        })
        .collect();

    for operation in analysis.operations.values_mut() {
        if let Some(request_body) = &mut operation.request_body {
            rewrite_request_body_schema_name(request_body, &aliases);
        }
        for schema_name in operation.response_schemas.values_mut() {
            *schema_name = renamed_schema_name(schema_name, &aliases);
        }
        for parameter in &mut operation.parameters {
            if let Some(schema_name) = &mut parameter.schema_ref {
                *schema_name = renamed_schema_name(schema_name, &aliases);
            }
            if let Some(serialization) = &mut parameter.query_serialization {
                rewrite_query_serialization_schema_names(serialization, &aliases);
            }
        }
    }

    for responses in analysis.operation_responses.values_mut() {
        for response in responses.values_mut() {
            if let Some(schema_name) = &mut response.schema_name {
                *schema_name = renamed_schema_name(schema_name, &aliases);
            }
            if let Some(OperationResponseBody::Json { schema_name, .. }) = &mut response.body {
                *schema_name = renamed_schema_name(schema_name, &aliases);
            }
        }
    }
}

fn renamed_schema_name(name: &str, aliases: &BTreeMap<String, String>) -> String {
    aliases
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn rewrite_schema_type_names(schema_type: &mut SchemaType, aliases: &BTreeMap<String, String>) {
    match schema_type {
        SchemaType::Object {
            properties,
            additional_properties,
            ..
        } => {
            for property in properties.values_mut() {
                rewrite_schema_type_names(&mut property.schema_type, aliases);
            }
            if let ObjectAdditionalProperties::Typed { value_type } = additional_properties {
                rewrite_schema_type_names(value_type, aliases);
            }
        }
        SchemaType::DiscriminatedUnion { variants, .. } => {
            for variant in variants {
                variant.type_name = renamed_schema_name(&variant.type_name, aliases);
                variant.schema_ref = renamed_schema_name(&variant.schema_ref, aliases);
            }
        }
        SchemaType::Union { variants, .. } | SchemaType::Composition { schemas: variants } => {
            for variant in variants {
                variant.target = renamed_schema_name(&variant.target, aliases);
            }
        }
        SchemaType::Array { item_type } => rewrite_schema_type_names(item_type, aliases),
        SchemaType::Nullable { inner_type } => rewrite_schema_type_names(inner_type, aliases),
        SchemaType::Untyped { .. } => {}
        SchemaType::Tuple { element_types } => {
            for element_type in element_types {
                rewrite_schema_type_names(element_type, aliases);
            }
        }
        SchemaType::Reference { target } => {
            *target = renamed_schema_name(target, aliases);
        }
        SchemaType::Primitive { .. }
        | SchemaType::StringEnum { .. }
        | SchemaType::ExtensibleEnum { .. } => {}
    }
}

fn rewrite_request_body_schema_name(
    request_body: &mut RequestBodyContent,
    aliases: &BTreeMap<String, String>,
) {
    match request_body {
        RequestBodyContent::Json { schema_name, .. }
        | RequestBodyContent::FormUrlEncoded { schema_name, .. }
        | RequestBodyContent::Multipart { schema_name, .. } => {
            *schema_name = renamed_schema_name(schema_name, aliases);
        }
        _ => {}
    }
}

fn rewrite_query_serialization_schema_names(
    serialization: &mut QuerySerialization,
    aliases: &BTreeMap<String, String>,
) {
    match serialization {
        QuerySerialization::FormExplodedArray { item_type }
        | QuerySerialization::FormArray { item_type }
        | QuerySerialization::SimpleHeaderArray { item_type } => {
            rewrite_array_item_type_schema_names(item_type, aliases);
        }
        QuerySerialization::FormExplodedNestedObject { properties } => {
            for property in properties {
                rewrite_query_property_type_schema_names(&mut property.value_type, aliases);
            }
        }
        _ => {}
    }
}

fn rewrite_array_item_type_schema_names(
    item_type: &mut ArrayItemType,
    aliases: &BTreeMap<String, String>,
) {
    match item_type {
        ArrayItemType::SchemaRef(name) => *name = renamed_schema_name(name, aliases),
        ArrayItemType::FlatStructRef {
            schema_name,
            properties,
        }
        | ArrayItemType::NestedStructRef {
            schema_name,
            properties,
        } => {
            *schema_name = renamed_schema_name(schema_name, aliases);
            for property in properties {
                rewrite_query_property_type_schema_names(&mut property.value_type, aliases);
            }
        }
        ArrayItemType::Scalar(_) => {}
    }
}

fn rewrite_query_property_type_schema_names(
    property_type: &mut QueryStructPropertyType,
    aliases: &BTreeMap<String, String>,
) {
    match property_type {
        QueryStructPropertyType::Array { item_type } => {
            rewrite_array_item_type_schema_names(item_type, aliases)
        }
        QueryStructPropertyType::Object { properties } => {
            for property in properties {
                rewrite_query_property_type_schema_names(&mut property.value_type, aliases);
            }
        }
        QueryStructPropertyType::Scalar(_) => {}
    }
}

fn rewrite_component_schema_references(value: &mut Value, aliases: &BTreeMap<String, String>) {
    match value {
        Value::Array(values) => {
            for value in values {
                rewrite_component_schema_references(value, aliases);
            }
        }
        Value::Object(object) => {
            if let Some(Value::String(reference)) = object.get_mut("$ref") {
                rewrite_component_schema_reference(reference, aliases);
            }

            if let Some(Value::Object(mapping)) = object.get_mut("mapping") {
                for target_value in mapping.values_mut() {
                    let Some(target) = target_value.as_str() else {
                        continue;
                    };
                    let replacement = aliases.get(target).cloned().or_else(|| {
                        let mut target = target.to_string();
                        rewrite_component_schema_reference(&mut target, aliases).then_some(target)
                    });
                    if let Some(replacement) = replacement {
                        *target_value = Value::String(replacement);
                    }
                }
            }

            for value in object.values_mut() {
                rewrite_component_schema_references(value, aliases);
            }
        }
        _ => {}
    }
}

fn rewrite_component_schema_reference(
    reference: &mut String,
    aliases: &BTreeMap<String, String>,
) -> bool {
    const PREFIX: &str = "#/components/schemas/";
    let Some(encoded_name) = reference.strip_prefix(PREFIX) else {
        return false;
    };
    let encoded_name = encoded_name.split('/').next().unwrap_or(encoded_name);

    for (source, replacement) in aliases {
        let encoded_source = source.replace('~', "~0").replace('/', "~1");
        if encoded_name == encoded_source {
            reference.replace_range(
                PREFIX.len()..PREFIX.len() + encoded_source.len(),
                replacement,
            );
            return true;
        }
    }

    false
}
