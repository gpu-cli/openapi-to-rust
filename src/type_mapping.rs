//! Centralized OpenAPI type → Rust type mapping.
//!
//! [`TypeMapper`] is the single chokepoint for every `(openapi_type,
//! format)` → Rust-type decision. Q2.0 introduced the chokepoint with
//! pass-through behavior; Q2 (quq) flips the defaults so common string
//! formats (`date-time`, `uuid`, `uri`, …) become typed Rust scalars
//! out of the box.
//!
//! # Design
//! - Per-format **strategy enums** (e.g. [`DateStrategy`]) drive the
//!   mapping. Defaults are opt-out: typed by default, set the
//!   strategy to `String` to recover plain `String`.
//! - [`MappedType`] carries the Rust type **plus** an optional
//!   `#[serde(with = "...")]` codec hint. Codec hints flow through
//!   [`SchemaType::Primitive`](crate::analysis::SchemaType::Primitive)
//!   to the field-emission site in `generator.rs`, which wraps them
//!   in a `#[serde(with = …)]` attribute.
//! - [`UsedFeatures`] records typed-scalar crate usage for helper emission and
//!   compatibility APIs. The complete generated dependency manifest is
//!   collected from the exact emitted files after operation/model pruning.
//!
//! # Conservative mode
//! Pass `TypeMappingConfig::conservative()` (CLI: `--types-conservative`)
//! to recover pre-Q2 behavior — every format renders as `String`. Useful
//! for bisecting regressions caused by typed-scalar adoption.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::openapi::{SchemaDetails, SchemaType as OpenApiSchemaType};

/// Result of mapping an OpenAPI `(type, format)` pair to a Rust type.
#[derive(Debug, Clone)]
pub struct MappedType {
    /// The Rust type as a string, e.g. `"String"`,
    /// `"chrono::DateTime<chrono::Utc>"`.
    pub rust_type: String,
    /// Optional `#[serde(with = "...")]` codec path. The generator
    /// wraps this in a `with = "<value>"` field attribute.
    pub serde_with: Option<String>,
    /// Optional crate this mapping introduced, tracked in [`UsedFeatures`].
    pub feature: Option<TypeFeature>,
}

impl MappedType {
    /// Construct a plain mapping with no codec and no external crate.
    pub fn plain(rust_type: impl Into<String>) -> Self {
        Self {
            rust_type: rust_type.into(),
            serde_with: None,
            feature: None,
        }
    }

    /// Plain mapping that records a feature crate (e.g. for types like
    /// `std::net::Ipv4Addr` we don't need a codec but we don't need a
    /// crate either — this helper is for crates that derive `serde`
    /// directly on the type).
    pub fn with_feature(rust_type: impl Into<String>, feature: TypeFeature) -> Self {
        Self {
            rust_type: rust_type.into(),
            serde_with: None,
            feature: Some(feature),
        }
    }

    /// Mapping that requires a `#[serde(with = ...)]` codec.
    pub fn with_codec(
        rust_type: impl Into<String>,
        codec_path: impl Into<String>,
        feature: TypeFeature,
    ) -> Self {
        Self {
            rust_type: rust_type.into(),
            serde_with: Some(codec_path.into()),
            feature: Some(feature),
        }
    }
}

/// Identifies an optional crate a mapping introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeFeature {
    Chrono,
    Time,
    /// `time::Date` via the generated `time_date_format` codec.
    /// Tracked separately from [`TypeFeature::Time`] so the
    /// generator only emits the `time::serde::format_description!`
    /// helper when a `format: date` field actually exists.
    TimeDate,
    /// `time::Time` via the generated `time_time_format` codec.
    TimeTime,
    Iso8601,
    Uuid,
    Bytes,
    Base64,
    Url,
    EmailAddress,
}

impl TypeFeature {
    /// Canonical dependency requirement for this typed scalar.
    pub fn dep_requirement(self) -> DepRequirement {
        match self {
            Self::Chrono => DepRequirement::new("chrono", "0.4").with_features(&["serde"]),
            // `serde` alone doesn't enable `time::serde::rfc3339`;
            // the codec modules are gated on formatting/parsing.
            Self::Time => DepRequirement::new("time", "0.3").with_features(&[
                "serde",
                "formatting",
                "parsing",
            ]),
            // `macros` on top: Date/Time have no built-in serde
            // codec, so the generated code declares one via
            // `time::serde::format_description!`.
            Self::TimeDate | Self::TimeTime => DepRequirement::new("time", "0.3").with_features(&[
                "serde",
                "formatting",
                "parsing",
                "macros",
            ]),
            Self::Iso8601 => DepRequirement::new("iso8601", "0.6").with_features(&["serde"]),
            Self::Uuid => DepRequirement::new("uuid", "1").with_features(&["serde"]),
            Self::Bytes => DepRequirement::new("bytes", "1").with_features(&["serde"]),
            Self::Base64 => DepRequirement::new("base64", "0.22"),
            Self::Url => DepRequirement::new("url", "2").with_features(&["serde"]),
            Self::EmailAddress => DepRequirement::new("email_address", "0.2"),
        }
    }
}

/// One crate the generated code needs in its `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepRequirement {
    pub crate_name: &'static str,
    pub version: &'static str,
    pub features: Vec<&'static str>,
    pub default_features: bool,
    pub optional: bool,
}

impl DepRequirement {
    pub fn new(crate_name: &'static str, version: &'static str) -> Self {
        Self {
            crate_name,
            version,
            features: Vec::new(),
            default_features: true,
            optional: false,
        }
    }

    pub fn with_features(mut self, features: &[&'static str]) -> Self {
        self.features = features.to_vec();
        self.features.sort_unstable();
        self.features.dedup();
        self
    }

    pub fn without_default_features(mut self) -> Self {
        self.default_features = false;
        self
    }

    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Render as a single TOML `[dependencies]` line. Picks the
    /// most compact form that still expresses the required features.
    pub fn to_toml_line(&self) -> String {
        if self.features.is_empty() && self.default_features && !self.optional {
            format!("{} = \"{}\"", self.crate_name, self.version)
        } else {
            let feats = self
                .features
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let mut attributes = vec![format!("version = \"{}\"", self.version)];
            if !self.default_features {
                attributes.push("default-features = false".to_string());
            }
            if !self.features.is_empty() {
                attributes.push(format!("features = [{feats}]"));
            }
            if self.optional {
                attributes.push("optional = true".to_string());
            }
            format!("{} = {{ {} }}", self.crate_name, attributes.join(", "))
        }
    }
}

/// Render `REQUIRED_DEPS.toml` content from a sorted set of
/// requirements. Returns `None` when the input is empty so the
/// caller can skip writing the file when no generated Rust files need
/// external crates.
pub fn render_required_deps_toml(deps: &[DepRequirement]) -> Option<String> {
    if deps.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str(
        "# Generated by openapi-to-rust.\n\
         # Complete direct dependencies for this generated output.\n\
         # Append this fragment to the consuming crate's Cargo.toml, or\n\
         # merge it with existing dependency and feature sections.\n\
         \n\
         [dependencies]\n",
    );
    for dep in deps {
        out.push_str(&dep.to_toml_line());
        out.push('\n');
    }
    if deps.iter().any(|dep| dep.crate_name == "specta") {
        out.push_str("\n[features]\nspecta = [\"dep:specta\"]\n");
    }
    Some(out)
}

/// Merge requirements by crate name, unioning features deterministically.
/// A dependency is optional only when every occurrence is optional, and
/// default features are enabled when any occurrence needs them.
pub fn merge_dep_requirements(
    requirements: impl IntoIterator<Item = DepRequirement>,
) -> Vec<DepRequirement> {
    let mut merged: std::collections::BTreeMap<&'static str, DepRequirement> =
        std::collections::BTreeMap::new();
    for mut dependency in requirements {
        dependency.features.sort_unstable();
        dependency.features.dedup();
        match merged.get_mut(dependency.crate_name) {
            Some(existing) => {
                debug_assert_eq!(existing.version, dependency.version);
                existing.default_features |= dependency.default_features;
                existing.optional &= dependency.optional;
                existing.features.extend(dependency.features);
                existing.features.sort_unstable();
                existing.features.dedup();
            }
            None => {
                merged.insert(dependency.crate_name, dependency);
            }
        }
    }
    merged.into_values().collect()
}

/// Collect the complete direct dependency set from the exact Rust files that
/// will be written. Scanning emitted paths keeps model pruning and operation
/// selection authoritative: dependencies cannot leak in from schemas or
/// operations that were analyzed but not generated.
pub fn collect_generated_dep_requirements<'a>(
    contents: impl IntoIterator<Item = &'a str>,
    enable_specta: bool,
) -> Vec<DepRequirement> {
    let generated = contents.into_iter().collect::<Vec<_>>().join("\n");
    let mut dependencies = Vec::new();
    let uses = |needle: &str| generated.contains(needle);

    if uses("serde::") {
        dependencies.push(DepRequirement::new("serde", "1").with_features(&["derive"]));
    }
    if uses("serde_json::") {
        dependencies.push(DepRequirement::new("serde_json", "1"));
    }
    if uses("serde_urlencoded::") {
        dependencies.push(DepRequirement::new("serde_urlencoded", "0.7"));
    }
    if uses("chrono::") {
        dependencies.push(TypeFeature::Chrono.dep_requirement());
    }
    let uses_time = uses("time::OffsetDateTime") || uses("time::Date") || uses("time::Time");
    if uses_time {
        let feature = if uses("time::Date") || uses("time::Time") {
            TypeFeature::TimeDate
        } else {
            TypeFeature::Time
        };
        dependencies.push(feature.dep_requirement());
    }
    if uses("iso8601::") {
        dependencies.push(TypeFeature::Iso8601.dep_requirement());
    }
    if uses("uuid::") {
        dependencies.push(TypeFeature::Uuid.dep_requirement());
    }
    if uses("bytes::") {
        dependencies.push(TypeFeature::Bytes.dep_requirement());
    }
    if uses("base64::") {
        dependencies.push(TypeFeature::Base64.dep_requirement());
    }
    if uses("url::") {
        let dependency = if uses("url::Url") {
            TypeFeature::Url.dep_requirement()
        } else {
            DepRequirement::new("url", "2")
        };
        dependencies.push(dependency);
    }
    if uses("email_address::") {
        dependencies.push(TypeFeature::EmailAddress.dep_requirement());
    }

    if uses("reqwest::") {
        let mut features = vec!["rustls-tls"];
        if uses(".json(&") {
            features.push("json");
        }
        // Multipart operations reference `reqwest::multipart::Form` directly.
        // reqwest is pinned with default-features off, so the feature has to be
        // requested explicitly or the generated client fails to compile with
        // "cannot find `multipart` in `reqwest`". The reqwest-middleware side
        // of this was already handled below; reqwest itself was missed
        // (openapi-generator-upz). Hits any spec with a file upload.
        if uses("reqwest::multipart") {
            features.push("multipart");
        }
        // `Response::bytes_stream()`, used by generated SSE operations, is
        // gated behind reqwest's `stream` feature.
        if uses(".bytes_stream()") {
            features.push("stream");
        }
        dependencies.push(
            DepRequirement::new("reqwest", "0.12")
                .without_default_features()
                .with_features(&features),
        );
    }
    if uses("reqwest_middleware::") {
        let dependency = if uses(".multipart(form)") {
            DepRequirement::new("reqwest-middleware", "0.4").with_features(&["multipart"])
        } else {
            DepRequirement::new("reqwest-middleware", "0.4")
        };
        dependencies.push(dependency);
    }
    if uses("reqwest_retry::") {
        let dependency = DepRequirement::new("reqwest-retry", "0.7");
        dependencies.push(if uses("reqwest_tracing::") {
            dependency
        } else {
            dependency.without_default_features()
        });
    }
    if uses("reqwest_tracing::") {
        dependencies.push(DepRequirement::new("reqwest-tracing", "0.5"));
    }
    if uses("reqwest_eventsource::") {
        dependencies.push(DepRequirement::new("reqwest-eventsource", "0.6"));
    }
    if uses("thiserror::") || uses("use thiserror::") {
        dependencies.push(DepRequirement::new("thiserror", "1"));
    }
    if uses("async_trait::") {
        dependencies.push(DepRequirement::new("async-trait", "0.1"));
    }
    if uses("futures_util::") {
        dependencies.push(DepRequirement::new("futures-util", "0.3"));
    }
    if uses("futures_core::") {
        dependencies.push(DepRequirement::new("futures-core", "0.3"));
    }
    if uses("use tracing::") {
        dependencies.push(DepRequirement::new("tracing", "0.1"));
    }
    if uses("axum::") {
        let mut features = vec!["json"];
        if uses("axum::response::sse::") {
            features.push("tokio");
        }
        dependencies.push(
            DepRequirement::new("axum", "0.8")
                .without_default_features()
                .with_features(&features),
        );
    }
    if uses("jsonschema::") {
        dependencies.push(DepRequirement::new("jsonschema", "0.49").without_default_features());
    }
    if uses("http_body_util::") {
        dependencies.push(DepRequirement::new("http-body-util", "0.1"));
    }
    if uses("mime::") {
        dependencies.push(DepRequirement::new("mime", "0.3"));
    }
    if enable_specta {
        let mut features = vec!["derive"];
        for (needle, feature) in [
            ("bytes::", "bytes"),
            ("chrono::", "chrono"),
            ("time::OffsetDateTime", "time"),
            ("url::Url", "url"),
            ("uuid::", "uuid"),
        ] {
            if uses(needle) {
                features.push(feature);
            }
        }
        if uses_time {
            features.push("time");
        }
        dependencies.push(
            DepRequirement::new("specta", "2.0.0-rc.25")
                .with_features(&features)
                .optional(),
        );
    }

    merge_dep_requirements(dependencies)
}

/// Snapshot a `UsedFeatures` set as a sorted, de-duplicated list of
/// `DepRequirement`s. Sorting by crate name keeps the emitted file
/// deterministic so it can be checked in or diffed.
pub fn collect_dep_requirements(used: &UsedFeatures) -> Vec<DepRequirement> {
    merge_dep_requirements(used.iter().map(|feature| feature.dep_requirement()))
}

/// Tracks which optional crates the generator emitted code for.
#[derive(Debug, Default, Clone)]
pub struct UsedFeatures {
    set: BTreeSet<TypeFeature>,
}

impl UsedFeatures {
    pub fn insert(&mut self, feature: TypeFeature) {
        self.set.insert(feature);
    }

    pub fn contains(&self, feature: TypeFeature) -> bool {
        self.set.contains(&feature)
    }

    pub fn iter(&self) -> impl Iterator<Item = &TypeFeature> {
        self.set.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

// =====================================================================
// Strategy enums
// =====================================================================

/// Strategy for `format: date-time | date | time`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DateStrategy {
    /// Plain `String`. Pre-Q2 behavior; pick this to opt out.
    String,
    /// `chrono::DateTime<Utc>` / `NaiveDate` / `NaiveTime` (default).
    #[default]
    Chrono,
    /// `time::OffsetDateTime` / `Date` / `Time`.
    Time,
}

/// Strategy for `format: duration` (ISO 8601 durations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DurationStrategy {
    // Off by default — `format: duration` is ISO 8601 (e.g.
    // "PT1H30M") but `chrono::Duration`'s native serde encodes
    // seconds. Round-tripping requires a custom parser that we'll
    // land in a follow-up; for now `duration` stays String so
    // default-on doesn't break specs that emit ISO 8601 strings
    // the chrono codec couldn't decode.
    #[default]
    String,
    /// `chrono::Duration`. Round-trips ISO 8601 durations via a
    /// small custom serde module emitted into the generated crate.
    Chrono,
    /// `iso8601::Duration` from the `iso8601` crate.
    Iso8601,
}

/// Strategy for `format: uuid` (or normalized aliases).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UuidStrategy {
    String,
    /// `uuid::Uuid` (default).
    #[default]
    Uuid,
}

/// Strategy for `format: byte` (base64-encoded binary on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ByteStrategy {
    String,
    /// `Vec<u8>` round-tripped via an inlined `base64_serde` module
    /// using the standard padded alphabet (default).
    #[default]
    Base64,
    /// `Vec<u8>` round-tripped with the URL-safe, unpadded alphabet
    /// from RFC 7515 section 2. This setting applies to every
    /// `format: byte` field in the generated module.
    Base64UrlUnpadded,
    /// `Vec<u8>` with no codec (caller responsible for encoding).
    VecU8,
}

/// Strategy for `format: binary` (raw octets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryStrategy {
    String,
    /// `bytes::Bytes` (default).
    #[default]
    Bytes,
    VecU8,
}

/// Strategy for `format: ipv4 | ipv6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IpStrategy {
    String,
    /// `std::net::Ipv4Addr` / `Ipv6Addr` (default; pure std, no deps).
    #[default]
    Std,
}

/// Strategy for `format: uri | url`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UriStrategy {
    String,
    /// `url::Url` (default).
    #[default]
    Url,
}

/// Strategy for `format: email`.
///
/// Email is **off by default** — the `email_address` crate is more
/// opinionated than the wire ever guarantees, and most APIs treat
/// emails as opaque strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailStrategy {
    #[default]
    String,
    EmailAddress,
}

// =====================================================================
// Top-level config
// =====================================================================

/// Configuration for [`TypeMapper`]. Mirrors the `[generator.types]`
/// TOML section. Defaults flip on every common typed scalar; opt out
/// per format by setting the strategy to `string` in TOML.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct TypeMappingConfig {
    pub date_time: DateStrategy,
    pub date: DateStrategy,
    pub time: DateStrategy,
    pub duration: DurationStrategy,
    pub uuid: UuidStrategy,
    pub byte: ByteStrategy,
    pub binary: BinaryStrategy,
    pub ipv4: IpStrategy,
    pub ipv6: IpStrategy,
    pub uri: UriStrategy,
    pub email: EmailStrategy,

    /// Q2.1: honor `format: uint32` / `uint64` integer formats and
    /// map them to `u32` / `u64` respectively. Default `true` (cheap,
    /// no extra crate). Set `false` to revert to the pre-Q2.1
    /// behavior where unsigned formats degraded to `i64`.
    #[serde(default = "default_true")]
    pub unsigned: bool,

    /// Q2.2: user-extensible format aliases applied before standard
    /// format dispatch (e.g. `"uuid4" -> "uuid"`,
    /// `"unix-time" -> "int64"`). Built-in defaults are merged with
    /// user-supplied entries; user entries win on collision.
    #[serde(default)]
    pub format_aliases: BTreeMap<String, String>,

    /// Object/array shape toggles. Filled in by Q2.3, Q2.5, Q2.7.
    pub shape: Option<TypeShapeConfig>,

    /// Constraint annotation mode. Filled in by Q2.4.
    pub constraints: Option<TypeConstraintsConfig>,

    /// Vendor-extension toggles for enums. Filled in by Q2.6.
    pub enums: Option<TypeEnumsConfig>,

    /// Rust type for `format: float`. Defaults to `f64`, which round-trips the
    /// JSON number the server actually sent; `f32` maps strictly by declared
    /// format at the cost of precision.
    #[serde(default)]
    pub float_precision: FloatPrecision,
}

/// How `format: float` is mapped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FloatPrecision {
    /// Map to `f64` (default). JSON carries no binary32, so widening preserves
    /// the transmitted value exactly.
    #[default]
    F64,
    /// Map to `f32`, matching the declared format literally. Values that are
    /// not representable in binary32 lose precision — `0.03` becomes
    /// `0.029999999329447746`.
    F32,
}

fn default_true() -> bool {
    true
}

impl Default for TypeMappingConfig {
    fn default() -> Self {
        Self {
            float_precision: FloatPrecision::default(),
            date_time: DateStrategy::default(),
            date: DateStrategy::default(),
            time: DateStrategy::default(),
            duration: DurationStrategy::default(),
            uuid: UuidStrategy::default(),
            byte: ByteStrategy::default(),
            binary: BinaryStrategy::default(),
            ipv4: IpStrategy::default(),
            ipv6: IpStrategy::default(),
            uri: UriStrategy::default(),
            email: EmailStrategy::default(),
            unsigned: true,
            format_aliases: BTreeMap::new(),
            shape: None,
            constraints: None,
            enums: None,
        }
    }
}

/// Built-in format aliases applied before user-supplied
/// [`TypeMappingConfig::format_aliases`]. These normalize common
/// vendor-isms found in real-world specs so the standard format
/// dispatch in [`TypeMapper::string_format`] /
/// [`TypeMapper::integer_format`] sees canonical names.
fn builtin_format_aliases() -> &'static [(&'static str, &'static str)] {
    &[
        ("uuid4", "uuid"),
        ("uuid_v4", "uuid"),
        ("UUID", "uuid"),
        ("unix-time", "int64"),
        ("unix_time", "int64"),
        ("unixtime", "int64"),
        ("timestamp", "int64"),
    ]
}

impl TypeMappingConfig {
    /// Q2.4: constraint-doc emission mode. Defaults to
    /// [`ConstraintMode::Doc`] when the
    /// `[generator.types.constraints]` block is absent or its
    /// `mode` field is unset.
    pub fn constraint_mode(&self) -> ConstraintMode {
        self.constraints
            .as_ref()
            .and_then(|c| c.mode)
            .unwrap_or_default()
    }

    /// Q2.6: should `x-enum-varnames` override the heuristic
    /// PascalCase variant naming? Default true.
    pub fn x_enum_varnames_enabled(&self) -> bool {
        self.enums
            .as_ref()
            .and_then(|e| e.x_enum_varnames)
            .unwrap_or(true)
    }

    /// Q2.6: should `x-enum-descriptions` emit per-variant doc
    /// comments? Default true.
    pub fn x_enum_descriptions_enabled(&self) -> bool {
        self.enums
            .as_ref()
            .and_then(|e| e.x_enum_descriptions)
            .unwrap_or(true)
    }

    /// Pre-Q2 behavior — every format renders as `String` and
    /// integer formats degrade to `i64`. Users opt in via
    /// `--types-conservative` when bisecting regressions introduced
    /// by typed-scalar adoption.
    pub fn conservative() -> Self {
        Self {
            // Conservative mode reproduces pre-Q2 output, which mapped
            // `format: float` literally to `f32`.
            float_precision: FloatPrecision::F32,
            date_time: DateStrategy::String,
            date: DateStrategy::String,
            time: DateStrategy::String,
            duration: DurationStrategy::String,
            uuid: UuidStrategy::String,
            byte: ByteStrategy::String,
            binary: BinaryStrategy::String,
            ipv4: IpStrategy::String,
            ipv6: IpStrategy::String,
            uri: UriStrategy::String,
            email: EmailStrategy::String,
            unsigned: false,
            format_aliases: BTreeMap::new(),
            shape: None,
            constraints: None,
            enums: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct TypeShapeConfig {
    pub additional_properties_typed: Option<bool>,
    pub unique_items_to_set: Option<bool>,
    pub primitive_unions: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct TypeConstraintsConfig {
    /// Q2.4 constraint annotation mode. Defaults to `Doc` when the
    /// `[generator.types.constraints]` block is absent (see
    /// [`TypeMapper::config_constraint_mode`]).
    pub mode: Option<ConstraintMode>,
}

/// Q2.4 — what to emit for OpenAPI constraint keywords
/// (`minimum`/`maximum`/`minLength`/`maxLength`/`pattern`/etc.).
///
/// **No client-side validation.** Constraints belong to the wire
/// contract; the server is the source of truth. The generator
/// surfaces them only as doc-comments so callers see the rules
/// without the SDK duplicating server logic and going brittle
/// when the rules drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintMode {
    /// Drop constraints entirely (pre-Q2.4 behavior).
    Off,
    /// Emit `/// Constraint: ...` doc comments on each field.
    /// Cheap, no extra crate dependency. Default.
    #[default]
    Doc,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct TypeEnumsConfig {
    pub x_enum_varnames: Option<bool>,
    pub x_enum_descriptions: Option<bool>,
}

// =====================================================================
// TypeMapper
// =====================================================================

pub struct TypeMapper {
    config: TypeMappingConfig,
    used: RefCell<UsedFeatures>,
}

impl Default for TypeMapper {
    fn default() -> Self {
        Self::new(TypeMappingConfig::default())
    }
}

impl TypeMapper {
    pub fn new(config: TypeMappingConfig) -> Self {
        Self {
            config,
            used: RefCell::new(UsedFeatures::default()),
        }
    }

    /// Snapshot of typed-scalar crates this mapper has referenced.
    pub fn used_features(&self) -> UsedFeatures {
        self.used.borrow().clone()
    }

    /// Borrow the underlying type-mapping config — useful for
    /// non-format-mapping toggles (`shape`, `enums`, `constraints`)
    /// that other modules need to inspect.
    pub fn config(&self) -> &TypeMappingConfig {
        &self.config
    }

    /// Q2.7 helper: should `anyOf` of primitives become an untagged
    /// enum with primitive variant types directly (true), or fall
    /// back to the pre-Q2.7 type-alias-per-variant shape (false)?
    /// Default: true.
    pub fn config_shape_primitive_unions(&self) -> Option<bool> {
        self.config.shape.as_ref().and_then(|s| s.primitive_unions)
    }

    /// Q2.3 helper: should `additionalProperties: <schema>` produce
    /// `BTreeMap<String, T>` (true) or degrade to `BTreeMap<String,
    /// serde_json::Value>` (false)? Default: true.
    pub fn config_shape_additional_properties_typed(&self) -> Option<bool> {
        self.config
            .shape
            .as_ref()
            .and_then(|s| s.additional_properties_typed)
    }

    /// Q2.4 helper: which constraint-annotation mode is active?
    /// Defaults to [`ConstraintMode::Doc`] when the
    /// `[generator.types.constraints]` block is absent or its `mode`
    /// field is unset.
    pub fn config_constraint_mode(&self) -> ConstraintMode {
        self.config
            .constraints
            .as_ref()
            .and_then(|c| c.mode)
            .unwrap_or_default()
    }

    fn record(&self, feature: TypeFeature) {
        self.used.borrow_mut().insert(feature);
    }

    /// Map `string` + optional `format` → typed Rust scalar.
    ///
    /// Routing:
    /// 1. Apply user-provided + built-in `format_aliases`.
    /// 2. Dispatch on the normalized format.
    /// 3. Honor each format's strategy in `self.config`.
    /// 4. Record any introduced crate in `used_features`.
    pub fn string_format(&self, format: Option<&str>) -> MappedType {
        let normalized = self.normalize_format(format);
        match normalized.as_deref() {
            Some("date-time") => self.map_date_time(self.config.date_time),
            Some("date") => self.map_date(self.config.date),
            Some("time") => self.map_time(self.config.time),
            Some("duration") => self.map_duration(self.config.duration),
            Some("uuid") => self.map_uuid(self.config.uuid),
            Some("byte") => self.map_byte(self.config.byte),
            Some("binary") => self.map_binary(self.config.binary),
            Some("ipv4") => self.map_ipv4(self.config.ipv4),
            Some("ipv6") => self.map_ipv6(self.config.ipv6),
            Some("uri") | Some("url") => self.map_uri(self.config.uri),
            Some("email") => self.map_email(self.config.email),
            // Unknown formats (hostname, password, idn-email, etc.)
            // and the no-format case fall through to plain String.
            _ => MappedType::plain("String"),
        }
    }

    /// Apply user + built-in format aliases (in that order — user
    /// entries win on collision). Built-ins normalize common
    /// vendor-isms like `uuid4` → `uuid` and `unix-time` → `int64`
    /// so the standard format dispatch below sees canonical names.
    fn normalize_format(&self, format: Option<&str>) -> Option<String> {
        let raw = format?;
        if let Some(target) = self.config.format_aliases.get(raw) {
            return Some(target.clone());
        }
        for (from, to) in builtin_format_aliases() {
            if *from == raw {
                return Some((*to).to_string());
            }
        }
        Some(raw.to_string())
    }

    fn map_date_time(&self, strat: DateStrategy) -> MappedType {
        match strat {
            DateStrategy::String => MappedType::plain("String"),
            DateStrategy::Chrono => {
                self.record(TypeFeature::Chrono);
                // chrono::DateTime<Utc> with the `serde` feature
                // serializes as RFC 3339 by default and parses both
                // `Z` and `+HH:MM` offsets on input. No `with`
                // attribute required.
                MappedType::with_feature("chrono::DateTime<chrono::Utc>", TypeFeature::Chrono)
            }
            DateStrategy::Time => {
                self.record(TypeFeature::Time);
                MappedType::with_codec(
                    "time::OffsetDateTime",
                    "time::serde::rfc3339",
                    TypeFeature::Time,
                )
            }
        }
    }

    fn map_date(&self, strat: DateStrategy) -> MappedType {
        match strat {
            DateStrategy::String => MappedType::plain("String"),
            DateStrategy::Chrono => {
                self.record(TypeFeature::Chrono);
                // chrono derives serde via the `serde` feature; no
                // codec needed for NaiveDate (ISO 8601 by default).
                MappedType::with_feature("chrono::NaiveDate", TypeFeature::Chrono)
            }
            DateStrategy::Time => {
                self.record(TypeFeature::TimeDate);
                // `time::serde::iso8601` only supports
                // OffsetDateTime; `time_date_format` is a codec
                // module the generator emits into the file via
                // `time::serde::format_description!` (GH #25).
                MappedType::with_codec("time::Date", "time_date_format", TypeFeature::TimeDate)
            }
        }
    }

    fn map_time(&self, strat: DateStrategy) -> MappedType {
        match strat {
            DateStrategy::String => MappedType::plain("String"),
            DateStrategy::Chrono => {
                self.record(TypeFeature::Chrono);
                MappedType::with_feature("chrono::NaiveTime", TypeFeature::Chrono)
            }
            DateStrategy::Time => {
                self.record(TypeFeature::TimeTime);
                // Same story as `time::Date`: no built-in codec, so
                // the generator emits `time_time_format`.
                MappedType::with_codec("time::Time", "time_time_format", TypeFeature::TimeTime)
            }
        }
    }

    fn map_duration(&self, strat: DurationStrategy) -> MappedType {
        match strat {
            DurationStrategy::String => MappedType::plain("String"),
            DurationStrategy::Chrono => {
                // Placeholder: chrono::Duration's native serde
                // encodes seconds (not ISO 8601). A follow-up will
                // emit an iso8601_duration_serde helper module and
                // wire it via with_codec; for now downgrade to the
                // String mapping so this strategy is safe to enable
                // even before the helper exists.
                MappedType::plain("String")
            }
            DurationStrategy::Iso8601 => {
                self.record(TypeFeature::Iso8601);
                MappedType::with_feature("iso8601::Duration", TypeFeature::Iso8601)
            }
        }
    }

    fn map_uuid(&self, strat: UuidStrategy) -> MappedType {
        match strat {
            UuidStrategy::String => MappedType::plain("String"),
            UuidStrategy::Uuid => {
                self.record(TypeFeature::Uuid);
                MappedType::with_feature("uuid::Uuid", TypeFeature::Uuid)
            }
        }
    }

    fn map_byte(&self, strat: ByteStrategy) -> MappedType {
        match strat {
            ByteStrategy::String => MappedType::plain("String"),
            ByteStrategy::VecU8 => MappedType::plain("Vec<u8>"),
            ByteStrategy::Base64 | ByteStrategy::Base64UrlUnpadded => {
                self.record(TypeFeature::Base64);
                // Path is resolved relative to the generated
                // module; the helper module is emitted as
                // `base64_serde` at the top of `types.rs`. Its
                // alphabet is selected once during code generation.
                MappedType::with_codec("Vec<u8>", "base64_serde", TypeFeature::Base64)
            }
        }
    }

    fn map_binary(&self, strat: BinaryStrategy) -> MappedType {
        match strat {
            BinaryStrategy::String => MappedType::plain("String"),
            BinaryStrategy::VecU8 => MappedType::plain("Vec<u8>"),
            BinaryStrategy::Bytes => {
                self.record(TypeFeature::Bytes);
                MappedType::with_feature("bytes::Bytes", TypeFeature::Bytes)
            }
        }
    }

    fn map_ipv4(&self, strat: IpStrategy) -> MappedType {
        match strat {
            IpStrategy::String => MappedType::plain("String"),
            IpStrategy::Std => MappedType::plain("std::net::Ipv4Addr"),
        }
    }

    fn map_ipv6(&self, strat: IpStrategy) -> MappedType {
        match strat {
            IpStrategy::String => MappedType::plain("String"),
            IpStrategy::Std => MappedType::plain("std::net::Ipv6Addr"),
        }
    }

    fn map_uri(&self, strat: UriStrategy) -> MappedType {
        match strat {
            UriStrategy::String => MappedType::plain("String"),
            UriStrategy::Url => {
                self.record(TypeFeature::Url);
                MappedType::with_feature("url::Url", TypeFeature::Url)
            }
        }
    }

    fn map_email(&self, strat: EmailStrategy) -> MappedType {
        match strat {
            EmailStrategy::String => MappedType::plain("String"),
            EmailStrategy::EmailAddress => {
                self.record(TypeFeature::EmailAddress);
                MappedType::with_feature("email_address::EmailAddress", TypeFeature::EmailAddress)
            }
        }
    }

    /// Map `integer` + optional `format` → Rust type.
    ///
    /// Q2.1: honors `uint32` / `uint64` (and a few vendor variants
    /// like `uint`) when `config.unsigned` is true (default).
    /// Setting `unsigned = false` reverts to the pre-Q2.1 behavior
    /// where unsigned formats degrade to `i64`.
    pub fn integer_format(&self, format: Option<&str>) -> MappedType {
        let normalized = self.normalize_format(format);
        match normalized.as_deref() {
            Some("int32") => MappedType::plain("i32"),
            Some("int64") => MappedType::plain("i64"),
            Some("uint32") if self.config.unsigned => MappedType::plain("u32"),
            Some("uint64") if self.config.unsigned => MappedType::plain("u64"),
            // OAS-adjacent specs sometimes use bare `uint` — treat
            // it as 64-bit unsigned to match the broadest intended
            // domain.
            Some("uint") if self.config.unsigned => MappedType::plain("u64"),
            _ => MappedType::plain("i64"),
        }
    }

    /// Map `number` + optional `format` → Rust type.
    ///
    /// `format: float` maps to `f64` by default rather than `f32`. JSON has no
    /// binary32: a value written on the wire as `0.03` parses losslessly into
    /// `f64`, but through `f32` it becomes `0.029999999329447746`. The declared
    /// format describes the server's internal storage, not the transport, so
    /// `f32` discards precision the response actually carried. Observed live on
    /// RunPod's catalog prices, which declare `float` while the billing
    /// endpoints declare `double`.
    ///
    /// Set `float_precision = "f32"` under `[generator.types]` to map strictly
    /// by declared format instead.
    pub fn number_format(&self, format: Option<&str>) -> MappedType {
        let normalized = self.normalize_format(format);
        match normalized.as_deref() {
            Some("float") if self.config.float_precision == FloatPrecision::F32 => {
                MappedType::plain("f32")
            }
            Some("float") => MappedType::plain("f64"),
            Some("double") => MappedType::plain("f64"),
            _ => MappedType::plain("f64"),
        }
    }

    pub fn boolean(&self) -> MappedType {
        MappedType::plain("bool")
    }

    pub fn untyped_array(&self) -> MappedType {
        MappedType::plain("Vec<serde_json::Value>")
    }

    pub fn dynamic_json(&self) -> MappedType {
        MappedType::plain("serde_json::Value")
    }

    pub fn null_unit(&self) -> MappedType {
        MappedType::plain("()")
    }

    /// One-shot dispatch from `(OpenApiSchemaType, &SchemaDetails)`.
    pub fn map(&self, ty: OpenApiSchemaType, details: &SchemaDetails) -> MappedType {
        let format = details.format.as_deref();
        match ty {
            OpenApiSchemaType::String => self.string_format(format),
            OpenApiSchemaType::Integer => self.integer_format(format),
            OpenApiSchemaType::Number => self.number_format(format),
            OpenApiSchemaType::Boolean => self.boolean(),
            OpenApiSchemaType::Array => self.untyped_array(),
            OpenApiSchemaType::Object => self.dynamic_json(),
            OpenApiSchemaType::Null => self.null_unit(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn details_with_format(format: Option<&str>) -> SchemaDetails {
        SchemaDetails {
            format: format.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn default_mapper_emits_typed_scalars_for_common_formats() {
        let m = TypeMapper::default();
        assert_eq!(
            m.string_format(Some("date-time")).rust_type,
            "chrono::DateTime<chrono::Utc>"
        );
        assert_eq!(m.string_format(Some("date")).rust_type, "chrono::NaiveDate");
        assert_eq!(m.string_format(Some("uuid")).rust_type, "uuid::Uuid");
        assert_eq!(m.string_format(Some("uri")).rust_type, "url::Url");
        assert_eq!(
            m.string_format(Some("ipv4")).rust_type,
            "std::net::Ipv4Addr"
        );
        assert_eq!(m.string_format(Some("byte")).rust_type, "Vec<u8>");
        assert_eq!(m.string_format(Some("binary")).rust_type, "bytes::Bytes");
    }

    #[test]
    fn date_time_uses_default_chrono_serde() {
        // chrono::DateTime<Utc> with the `serde` feature serializes
        // as RFC 3339 by default — no `with = ...` codec required.
        let m = TypeMapper::default();
        let mt = m.string_format(Some("date-time"));
        assert_eq!(mt.rust_type, "chrono::DateTime<chrono::Utc>");
        assert!(mt.serde_with.is_none());
        assert_eq!(mt.feature, Some(TypeFeature::Chrono));
    }

    #[test]
    fn byte_emits_base64_codec() {
        let m = TypeMapper::default();
        let mt = m.string_format(Some("byte"));
        assert_eq!(mt.rust_type, "Vec<u8>");
        assert_eq!(mt.serde_with.as_deref(), Some("base64_serde"));
        assert_eq!(mt.feature, Some(TypeFeature::Base64));
    }

    #[test]
    fn byte_url_unpadded_reuses_base64_codec() {
        let mapper = TypeMapper::new(TypeMappingConfig {
            byte: ByteStrategy::Base64UrlUnpadded,
            ..TypeMappingConfig::default()
        });
        let mapped = mapper.string_format(Some("byte"));
        assert_eq!(mapped.rust_type, "Vec<u8>");
        assert_eq!(mapped.serde_with.as_deref(), Some("base64_serde"));
        assert_eq!(mapped.feature, Some(TypeFeature::Base64));
    }

    #[test]
    fn byte_url_unpadded_parses_from_toml() {
        let config: TypeMappingConfig =
            toml::from_str(r#"byte = "base64_url_unpadded""#).expect("parse type config");
        assert_eq!(config.byte, ByteStrategy::Base64UrlUnpadded);
    }

    #[test]
    fn conservative_config_collapses_everything_to_string() {
        let m = TypeMapper::new(TypeMappingConfig::conservative());
        for fmt in [
            Some("date-time"),
            Some("uuid"),
            Some("uri"),
            Some("byte"),
            Some("binary"),
            Some("ipv4"),
            Some("ipv6"),
            Some("date"),
            None,
        ] {
            let mt = m.string_format(fmt);
            assert_eq!(mt.rust_type, "String", "format = {fmt:?}");
            assert!(mt.serde_with.is_none(), "format = {fmt:?}");
        }
    }

    #[test]
    fn unknown_formats_fall_through_to_string() {
        let m = TypeMapper::default();
        for fmt in [Some("hostname"), Some("password"), Some("idn-email")] {
            assert_eq!(m.string_format(fmt).rust_type, "String");
        }
    }

    #[test]
    fn integer_formats_match_pre_refactor_behavior() {
        let m = TypeMapper::default();
        assert_eq!(m.integer_format(Some("int32")).rust_type, "i32");
        assert_eq!(m.integer_format(Some("int64")).rust_type, "i64");
        assert_eq!(m.integer_format(None).rust_type, "i64");
    }

    #[test]
    fn integer_formats_default_handles_unsigned_q21() {
        let m = TypeMapper::default();
        assert_eq!(m.integer_format(Some("uint32")).rust_type, "u32");
        assert_eq!(m.integer_format(Some("uint64")).rust_type, "u64");
        // Non-standard `uint` falls into the broader uint64 bucket.
        assert_eq!(m.integer_format(Some("uint")).rust_type, "u64");
    }

    #[test]
    fn unsigned_off_degrades_uint_to_i64() {
        let mut cfg = TypeMappingConfig::default();
        cfg.unsigned = false;
        let m = TypeMapper::new(cfg);
        assert_eq!(m.integer_format(Some("uint32")).rust_type, "i64");
        assert_eq!(m.integer_format(Some("uint64")).rust_type, "i64");
    }

    #[test]
    fn conservative_disables_unsigned() {
        let m = TypeMapper::new(TypeMappingConfig::conservative());
        assert_eq!(m.integer_format(Some("uint64")).rust_type, "i64");
    }

    #[test]
    fn builtin_aliases_normalize_uuid_variants_to_uuid() {
        let m = TypeMapper::default();
        for fmt in ["uuid4", "uuid_v4", "UUID"] {
            let mt = m.string_format(Some(fmt));
            assert_eq!(mt.rust_type, "uuid::Uuid", "format = {fmt}");
        }
    }

    #[test]
    fn builtin_aliases_normalize_unix_time_to_int64() {
        let m = TypeMapper::default();
        for fmt in ["unix-time", "unix_time", "unixtime", "timestamp"] {
            let mt = m.integer_format(Some(fmt));
            assert_eq!(mt.rust_type, "i64", "format = {fmt}");
        }
    }

    #[test]
    fn user_alias_overrides_builtin() {
        let mut cfg = TypeMappingConfig::default();
        // User wants `uuid4` to mean plain string instead of uuid.
        cfg.format_aliases
            .insert("uuid4".to_string(), "hostname".to_string());
        let m = TypeMapper::new(cfg);
        // hostname is unmapped → falls through to String.
        assert_eq!(m.string_format(Some("uuid4")).rust_type, "String");
    }

    #[test]
    fn used_features_records_referenced_crates() {
        let m = TypeMapper::default();
        let _ = m.string_format(Some("date-time"));
        let _ = m.string_format(Some("uuid"));
        let used = m.used_features();
        assert!(used.contains(TypeFeature::Chrono));
        assert!(used.contains(TypeFeature::Uuid));
        assert!(!used.contains(TypeFeature::Bytes));
    }

    #[test]
    fn format_alias_normalizes_before_dispatch() {
        let mut cfg = TypeMappingConfig::default();
        cfg.format_aliases
            .insert("uuid4".to_string(), "uuid".to_string());
        let m = TypeMapper::new(cfg);
        assert_eq!(m.string_format(Some("uuid4")).rust_type, "uuid::Uuid");
    }

    #[test]
    fn conservative_helper_round_trips() {
        let cfg = TypeMappingConfig::conservative();
        assert!(matches!(cfg.date_time, DateStrategy::String));
        assert!(matches!(cfg.uuid, UuidStrategy::String));
    }

    #[test]
    fn dep_requirement_renders_features_list() {
        let dep = TypeFeature::Chrono.dep_requirement();
        assert_eq!(dep.crate_name, "chrono");
        assert_eq!(dep.features, vec!["serde"]);
        assert_eq!(
            dep.to_toml_line(),
            r#"chrono = { version = "0.4", features = ["serde"] }"#
        );
    }

    #[test]
    fn dep_requirement_omits_features_when_none() {
        let dep = TypeFeature::Base64.dep_requirement();
        assert_eq!(dep.to_toml_line(), r#"base64 = "0.22""#);
    }

    #[test]
    fn collect_dep_requirements_is_sorted_and_unique() {
        let mut used = UsedFeatures::default();
        used.insert(TypeFeature::Url);
        used.insert(TypeFeature::Chrono);
        used.insert(TypeFeature::Chrono); // duplicate
        used.insert(TypeFeature::Uuid);
        let deps = collect_dep_requirements(&used);
        assert_eq!(
            deps.iter().map(|d| d.crate_name).collect::<Vec<_>>(),
            vec!["chrono", "url", "uuid"]
        );
    }

    #[test]
    fn render_required_deps_toml_is_none_when_empty() {
        let deps: Vec<DepRequirement> = Vec::new();
        assert!(render_required_deps_toml(&deps).is_none());
    }

    #[test]
    fn render_required_deps_toml_includes_dependencies_block() {
        let deps = vec![
            TypeFeature::Chrono.dep_requirement(),
            TypeFeature::Uuid.dep_requirement(),
        ];
        let toml = render_required_deps_toml(&deps).expect("non-empty");
        assert!(toml.contains("[dependencies]"));
        assert!(toml.contains("chrono = "));
        assert!(toml.contains("uuid = "));
        assert!(toml.contains("# Generated by openapi-to-rust"));
    }

    #[test]
    fn map_dispatches_through_helpers() {
        let m = TypeMapper::default();
        assert_eq!(
            m.map(
                OpenApiSchemaType::String,
                &details_with_format(Some("uuid"))
            )
            .rust_type,
            "uuid::Uuid"
        );
        assert_eq!(
            m.map(
                OpenApiSchemaType::Integer,
                &details_with_format(Some("int32"))
            )
            .rust_type,
            "i32"
        );
    }
}
