//! Centralized OpenAPI type → Rust type mapping.
//!
//! Q2.0 introduces this module as the single chokepoint for every
//! `(openapi_type, format)` → Rust-type decision. With the default
//! [`TypeMappingConfig`] every mapping is bit-identical to the pre-refactor
//! behavior; later Q2.* issues fill in real per-format strategies.
//!
//! # Why a chokepoint
//! Pre-Q2.0 the same logic lived in two places (`openapi_type_to_rust_type`
//! and `get_number_rust_type` in `analysis.rs`) plus a smattering of inline
//! `"String".to_string()` literals. Adding format-aware mappings (chrono,
//! uuid, …) without a chokepoint means touching every site for every
//! format. With [`TypeMapper`] each future issue edits one method.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::openapi::{SchemaDetails, SchemaType as OpenApiSchemaType};

/// Result of mapping an OpenAPI `(type, format)` pair to a Rust type.
///
/// For Q2.0 only `rust_type` is consumed by callers; `serde_with` and
/// `feature` are wired through so subsequent issues can populate them
/// without changing call sites.
#[derive(Debug, Clone)]
pub struct MappedType {
    /// The Rust type as a string, e.g. `"String"`, `"i64"`,
    /// `"chrono::DateTime<chrono::Utc>"`. Stored as a string because the
    /// rest of the generator threads types as strings until token
    /// generation in `generator.rs`.
    pub rust_type: String,
    /// Optional `#[serde(with = "...")]` codec hint to attach at the
    /// field-emission site. `None` for primitive types that need no codec.
    pub serde_with: Option<String>,
    /// Optional crate dependency this mapping introduces, tracked so the
    /// dep advisory (Q2.8) can list what the user must add to Cargo.toml.
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
}

/// Identifies an optional crate that a mapping introduced. Q2.0 defines the
/// enum so later issues can record their crate without re-shaping the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(dead_code)]
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

/// Tracks which optional crates the generator actually emitted code for.
/// Drives the REQUIRED_DEPS advisory in Q2.8. Held inside a `RefCell` so
/// `&TypeMapper` callers can record without taking `&mut`.
#[derive(Debug, Default, Clone)]
pub struct UsedFeatures {
    set: BTreeSet<TypeFeature>,
}

impl UsedFeatures {
    pub fn insert(&mut self, feature: TypeFeature) {
        self.set.insert(feature);
    }

    pub fn iter(&self) -> impl Iterator<Item = &TypeFeature> {
        self.set.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

/// Configuration for [`TypeMapper`]. Mirrors the `[generator.types]` TOML
/// section. Every field has a default that preserves pre-Q2.0 behavior.
///
/// Subsequent Q2.* issues flip these defaults to opt-out (per the agreed
/// design), but Q2.0 deliberately changes nothing — the snapshot suite
/// must produce a zero-byte diff after this refactor.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case")]
pub struct TypeMappingConfig {
    /// Strategy for `format: date-time`. Q2 (quq) will accept
    /// `"chrono"`, `"time"`, `"string"`. Q2.0 leaves it `None` and
    /// every value renders as `String`.
    pub date_time: Option<String>,
    pub date: Option<String>,
    pub time: Option<String>,
    pub duration: Option<String>,
    pub uuid: Option<String>,
    pub byte: Option<String>,
    pub binary: Option<String>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub uri: Option<String>,
    pub email: Option<String>,

    /// When true, `format: uint32`/`uint64` map to `u32`/`u64`.
    /// Q2.1 will flip the default to true; Q2.0 leaves it None
    /// which preserves today's i64 fallback.
    pub unsigned: Option<bool>,

    /// User-extensible aliases applied before standard format dispatch.
    /// Q2.2 introduces built-in defaults (`uuid4 → uuid`, `unix-time →
    /// int64`); Q2.0 leaves it empty.
    #[serde(default)]
    pub format_aliases: BTreeMap<String, String>,

    /// Object/array shape toggles. Filled in by Q2.3, Q2.5, Q2.7.
    pub shape: Option<TypeShapeConfig>,

    /// Constraint annotation mode. Filled in by Q2.4.
    pub constraints: Option<TypeConstraintsConfig>,

    /// Vendor-extension toggles for enums. Filled in by Q2.6.
    pub enums: Option<TypeEnumsConfig>,
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
    /// `"off"` | `"doc"` | `"validator_crate"`.
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "snake_case")]
pub struct TypeEnumsConfig {
    pub x_enum_varnames: Option<bool>,
    pub x_enum_descriptions: Option<bool>,
}

/// The single chokepoint for OpenAPI → Rust type decisions.
///
/// Construct one per generation run, thread it into [`SchemaAnalyzer`]
/// (via `with_type_mapper`), and call its mapping methods from any code
/// that previously inlined a `"String".to_string()` or similar literal.
///
/// [`SchemaAnalyzer`]: crate::analysis::SchemaAnalyzer
pub struct TypeMapper {
    #[allow(dead_code)] // Q2.* issues read this; Q2.0 only stores it.
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

    /// Snapshot of crates this mapper has emitted references to. Empty in
    /// Q2.0 because no mapping records a feature yet.
    pub fn used_features(&self) -> UsedFeatures {
        self.used.borrow().clone()
    }

    /// Map `string` + optional `format` → Rust type.
    ///
    /// Q2.0: always `String`. Q2 (quq) branches on `format` here.
    pub fn string_format(&self, _format: Option<&str>) -> MappedType {
        MappedType::plain("String")
    }

    /// Map `integer` + optional `format` → Rust type.
    ///
    /// Q2.0 preserves the pre-refactor semantics:
    /// `int32 → i32`, `int64 → i64`, anything else → `i64`.
    /// Q2.1 will additionally honor `uint32`/`uint64`.
    pub fn integer_format(&self, format: Option<&str>) -> MappedType {
        match format {
            Some("int32") => MappedType::plain("i32"),
            Some("int64") => MappedType::plain("i64"),
            _ => MappedType::plain("i64"),
        }
    }

    /// Map `number` + optional `format` → Rust type.
    ///
    /// Q2.0 preserves the pre-refactor semantics:
    /// `float → f32`, `double → f64`, anything else → `f64`.
    pub fn number_format(&self, format: Option<&str>) -> MappedType {
        match format {
            Some("float") => MappedType::plain("f32"),
            Some("double") => MappedType::plain("f64"),
            _ => MappedType::plain("f64"),
        }
    }

    pub fn boolean(&self) -> MappedType {
        MappedType::plain("bool")
    }

    /// Fallback for `array` schemas with no `items` definition.
    pub fn untyped_array(&self) -> MappedType {
        MappedType::plain("Vec<serde_json::Value>")
    }

    /// Fallback for `object` schemas the analyzer can't structurally
    /// describe (and dynamic-JSON object patterns).
    pub fn dynamic_json(&self) -> MappedType {
        MappedType::plain("serde_json::Value")
    }

    /// `null` openapi type.
    pub fn null_unit(&self) -> MappedType {
        MappedType::plain("()")
    }

    /// One-shot dispatch from `(OpenApiSchemaType, &SchemaDetails)`.
    /// Mirrors the pre-Q2.0 `openapi_type_to_rust_type` helper exactly.
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
    fn default_mapper_strings_collapse_to_string() {
        let m = TypeMapper::default();
        for fmt in [None, Some("date-time"), Some("uuid"), Some("uri")] {
            let mapped = m.string_format(fmt);
            assert_eq!(mapped.rust_type, "String");
            assert!(mapped.serde_with.is_none());
            assert!(mapped.feature.is_none());
        }
    }

    #[test]
    fn integer_formats_match_pre_refactor_behavior() {
        let m = TypeMapper::default();
        assert_eq!(m.integer_format(Some("int32")).rust_type, "i32");
        assert_eq!(m.integer_format(Some("int64")).rust_type, "i64");
        assert_eq!(m.integer_format(None).rust_type, "i64");
        assert_eq!(m.integer_format(Some("uint64")).rust_type, "i64");
    }

    #[test]
    fn number_formats_match_pre_refactor_behavior() {
        let m = TypeMapper::default();
        assert_eq!(m.number_format(Some("float")).rust_type, "f32");
        assert_eq!(m.number_format(Some("double")).rust_type, "f64");
        assert_eq!(m.number_format(None).rust_type, "f64");
    }

    #[test]
    fn map_dispatches_through_helpers() {
        let m = TypeMapper::default();
        assert_eq!(
            m.map(OpenApiSchemaType::String, &details_with_format(Some("date-time")))
                .rust_type,
            "String"
        );
        assert_eq!(
            m.map(OpenApiSchemaType::Integer, &details_with_format(Some("int32")))
                .rust_type,
            "i32"
        );
        assert_eq!(
            m.map(OpenApiSchemaType::Boolean, &details_with_format(None))
                .rust_type,
            "bool"
        );
        assert_eq!(
            m.map(OpenApiSchemaType::Array, &details_with_format(None))
                .rust_type,
            "Vec<serde_json::Value>"
        );
        assert_eq!(
            m.map(OpenApiSchemaType::Object, &details_with_format(None))
                .rust_type,
            "serde_json::Value"
        );
        assert_eq!(
            m.map(OpenApiSchemaType::Null, &details_with_format(None))
                .rust_type,
            "()"
        );
    }

    #[test]
    fn used_features_is_empty_in_q2_0() {
        let m = TypeMapper::default();
        let _ = m.string_format(Some("date-time"));
        let _ = m.integer_format(Some("int64"));
        assert!(m.used_features().is_empty());
    }
}
