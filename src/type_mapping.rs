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
//! - [`UsedFeatures`] tracks which optional crates the mapper
//!   actually emitted references to. Q2.8 will read this after
//!   generation and write a `REQUIRED_DEPS.toml`.
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
    /// Optional crate this mapping introduced. Tracked in
    /// [`UsedFeatures`] for the dep advisory (Q2.8).
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
    Iso8601,
    Uuid,
    Bytes,
    Base64,
    Url,
    EmailAddress,
    Validator,
}

impl TypeFeature {
    /// Canonical dependency line for this feature. Q2.8 uses this to
    /// emit `REQUIRED_DEPS.toml` next to the generated code so users
    /// know exactly which crates to add to their Cargo.toml.
    pub fn dep_requirement(self) -> DepRequirement {
        match self {
            Self::Chrono => DepRequirement::new("chrono", "0.4").with_features(&["serde"]),
            Self::Time => DepRequirement::new("time", "0.3").with_features(&["serde"]),
            Self::Iso8601 => DepRequirement::new("iso8601", "0.6"),
            Self::Uuid => DepRequirement::new("uuid", "1").with_features(&["serde", "v4"]),
            Self::Bytes => DepRequirement::new("bytes", "1").with_features(&["serde"]),
            Self::Base64 => DepRequirement::new("base64", "0.22"),
            Self::Url => DepRequirement::new("url", "2").with_features(&["serde"]),
            Self::EmailAddress => DepRequirement::new("email_address", "0.2"),
            Self::Validator => {
                DepRequirement::new("validator", "0.20").with_features(&["derive"])
            }
        }
    }
}

/// One crate the generated code needs in its `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepRequirement {
    pub crate_name: &'static str,
    pub version: &'static str,
    pub features: Vec<&'static str>,
}

impl DepRequirement {
    pub fn new(crate_name: &'static str, version: &'static str) -> Self {
        Self {
            crate_name,
            version,
            features: Vec::new(),
        }
    }

    pub fn with_features(mut self, features: &[&'static str]) -> Self {
        self.features = features.to_vec();
        self
    }

    /// Render as a single TOML `[dependencies]` line. Picks the
    /// most compact form that still expresses the required features.
    pub fn to_toml_line(&self) -> String {
        if self.features.is_empty() {
            format!("{} = \"{}\"", self.crate_name, self.version)
        } else {
            let feats = self
                .features
                .iter()
                .map(|f| format!("\"{f}\""))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} = {{ version = \"{}\", features = [{}] }}",
                self.crate_name, self.version, feats
            )
        }
    }
}

/// Render `REQUIRED_DEPS.toml` content from a sorted set of
/// requirements. Returns `None` when the input is empty so the
/// caller can skip writing the file (no clutter when no optional
/// crates were used).
pub fn render_required_deps_toml(deps: &[DepRequirement]) -> Option<String> {
    if deps.is_empty() {
        return None;
    }
    let mut out = String::new();
    out.push_str(
        "# Generated by openapi-to-rust.\n\
         # These crates are required by the typed-scalar formats used\n\
         # in your OpenAPI spec. Copy these lines into the [dependencies]\n\
         # section of your consuming crate's Cargo.toml.\n\
         #\n\
         # To opt out of typed scalars (and avoid these deps), set\n\
         # the relevant strategies to \"string\" in [generator.types],\n\
         # or pass --types-conservative on the CLI.\n\
         \n\
         [dependencies]\n",
    );
    for dep in deps {
        out.push_str(&dep.to_toml_line());
        out.push('\n');
    }
    Some(out)
}

/// Snapshot a `UsedFeatures` set as a sorted, de-duplicated list of
/// `DepRequirement`s. Sorting by crate name keeps the emitted file
/// deterministic so it can be checked in or diffed.
pub fn collect_dep_requirements(used: &UsedFeatures) -> Vec<DepRequirement> {
    let mut deps: Vec<DepRequirement> =
        used.iter().map(|f| f.dep_requirement()).collect();
    deps.sort_by_key(|d| d.crate_name);
    deps.dedup_by_key(|d| d.crate_name);
    deps
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DateStrategy {
    /// Plain `String`. Pre-Q2 behavior; pick this to opt out.
    String,
    /// `chrono::DateTime<Utc>` / `NaiveDate` / `NaiveTime` (default).
    Chrono,
    /// `time::OffsetDateTime` / `Date` / `Time`.
    Time,
}

impl Default for DateStrategy {
    fn default() -> Self {
        Self::Chrono
    }
}

/// Strategy for `format: duration` (ISO 8601 durations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DurationStrategy {
    String,
    /// `chrono::Duration` (default). Round-trips ISO 8601 durations
    /// via a small custom serde module emitted into the generated
    /// crate.
    Chrono,
    /// `iso8601::Duration` from the `iso8601` crate.
    Iso8601,
}

impl Default for DurationStrategy {
    fn default() -> Self {
        // Off by default — `format: duration` is ISO 8601 (e.g.
        // "PT1H30M") but `chrono::Duration`'s native serde encodes
        // seconds. Round-tripping requires a custom parser that
        // we'll land in a follow-up; for now `duration` stays
        // String so default-on doesn't break specs that emit ISO
        // 8601 strings the chrono codec couldn't decode.
        Self::String
    }
}

/// Strategy for `format: uuid` (or normalized aliases).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UuidStrategy {
    String,
    /// `uuid::Uuid` (default).
    Uuid,
}

impl Default for UuidStrategy {
    fn default() -> Self {
        Self::Uuid
    }
}

/// Strategy for `format: byte` (base64-encoded binary on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ByteStrategy {
    String,
    /// `Vec<u8>` round-tripped via an inlined `base64_serde` module
    /// (default).
    Base64,
    /// `Vec<u8>` with no codec (caller responsible for encoding).
    VecU8,
}

impl Default for ByteStrategy {
    fn default() -> Self {
        Self::Base64
    }
}

/// Strategy for `format: binary` (raw octets).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryStrategy {
    String,
    /// `bytes::Bytes` (default).
    Bytes,
    VecU8,
}

impl Default for BinaryStrategy {
    fn default() -> Self {
        Self::Bytes
    }
}

/// Strategy for `format: ipv4 | ipv6`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IpStrategy {
    String,
    /// `std::net::Ipv4Addr` / `Ipv6Addr` (default; pure std, no deps).
    Std,
}

impl Default for IpStrategy {
    fn default() -> Self {
        Self::Std
    }
}

/// Strategy for `format: uri | url`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UriStrategy {
    String,
    /// `url::Url` (default).
    Url,
}

impl Default for UriStrategy {
    fn default() -> Self {
        Self::Url
    }
}

/// Strategy for `format: email`.
///
/// Email is **off by default** — the `email_address` crate is more
/// opinionated than the wire ever guarantees, and most APIs treat
/// emails as opaque strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailStrategy {
    String,
    EmailAddress,
}

impl Default for EmailStrategy {
    fn default() -> Self {
        Self::String
    }
}

// =====================================================================
// Top-level config
// =====================================================================

/// Configuration for [`TypeMapper`]. Mirrors the `[generator.types]`
/// TOML section. Defaults flip on every common typed scalar; opt out
/// per format by setting the strategy to `string` in TOML.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case")]
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

    /// When true, `format: uint32`/`uint64` map to `u32`/`u64`. Q2.1
    /// will flip the default to true; Q2 leaves it None which
    /// preserves today's i64 fallback.
    pub unsigned: Option<bool>,

    /// User-extensible aliases applied before standard format
    /// dispatch. Q2.2 introduces built-in defaults.
    #[serde(default)]
    pub format_aliases: BTreeMap<String, String>,

    /// Object/array shape toggles. Filled in by Q2.3, Q2.5, Q2.7.
    pub shape: Option<TypeShapeConfig>,

    /// Constraint annotation mode. Filled in by Q2.4.
    pub constraints: Option<TypeConstraintsConfig>,

    /// Vendor-extension toggles for enums. Filled in by Q2.6.
    pub enums: Option<TypeEnumsConfig>,
}

impl TypeMappingConfig {
    /// Pre-Q2 behavior — every format renders as `String`. Users opt
    /// in via `--types-conservative` when bisecting regressions
    /// introduced by typed-scalar adoption.
    pub fn conservative() -> Self {
        Self {
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
            unsigned: None,
            format_aliases: BTreeMap::new(),
            shape: None,
            constraints: None,
            enums: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case")]
pub struct TypeShapeConfig {
    pub additional_properties_typed: Option<bool>,
    pub unique_items_to_set: Option<bool>,
    pub primitive_unions: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case")]
pub struct TypeConstraintsConfig {
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case")]
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

    /// Snapshot of crates this mapper has emitted references to.
    /// Read after generation by Q2.8 to write `REQUIRED_DEPS.toml`.
    pub fn used_features(&self) -> UsedFeatures {
        self.used.borrow().clone()
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

    /// Apply built-in + user-provided format aliases.
    /// Q2.0 has no built-ins; Q2.2 will add `uuid4 → uuid` and
    /// `unix-time → int64`.
    fn normalize_format(&self, format: Option<&str>) -> Option<String> {
        let raw = format?;
        if let Some(target) = self.config.format_aliases.get(raw) {
            return Some(target.clone());
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
                MappedType::with_feature(
                    "chrono::DateTime<chrono::Utc>",
                    TypeFeature::Chrono,
                )
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
                self.record(TypeFeature::Time);
                MappedType::with_codec(
                    "time::Date",
                    "time::serde::iso8601",
                    TypeFeature::Time,
                )
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
                self.record(TypeFeature::Time);
                MappedType::with_codec(
                    "time::Time",
                    "time::serde::iso8601",
                    TypeFeature::Time,
                )
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
            ByteStrategy::Base64 => {
                self.record(TypeFeature::Base64);
                // Path is resolved relative to the generated
                // module; the helper module is emitted as
                // `base64_serde` at the top of `types.rs`.
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
                MappedType::with_feature(
                    "email_address::EmailAddress",
                    TypeFeature::EmailAddress,
                )
            }
        }
    }

    /// Map `integer` + optional `format` → Rust type.
    /// Q2 (quq) keeps Q2.0 semantics; Q2.1 adds `uint32`/`uint64`.
    pub fn integer_format(&self, format: Option<&str>) -> MappedType {
        let normalized = self.normalize_format(format);
        match normalized.as_deref() {
            Some("int32") => MappedType::plain("i32"),
            Some("int64") => MappedType::plain("i64"),
            _ => MappedType::plain("i64"),
        }
    }

    pub fn number_format(&self, format: Option<&str>) -> MappedType {
        let normalized = self.normalize_format(format);
        match normalized.as_deref() {
            Some("float") => MappedType::plain("f32"),
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
