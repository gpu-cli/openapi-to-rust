//! Specification Extensions support — `x-*` fields per OAS §"Specification Extensions".
//!
//! `Extensions` is a flatten target that accepts only keys with the `x-` prefix.
//! Any other unknown key triggers a deserialize error, surfacing what the audit
//! found to be the architectural root cause: every spec struct silently
//! swallowed unknown fields into a generic `BTreeMap`.
//!
//! Use it on every spec struct that previously had
//! `#[serde(flatten)] pub extra: BTreeMap<String, Value>`:
//!
//! ```ignore
//! #[derive(Deserialize)]
//! struct Foo {
//!     name: String,
//!     #[serde(flatten, default)]
//!     extensions: Extensions,
//! }
//! ```
//!
//! Note: `Schema` and `SchemaDetails` are intentionally left with the loose
//! `extra: BTreeMap<...>` for now — analysis.rs reads JSON Schema 2020-12
//! keywords (`patternProperties`, `propertyNames`, `dependentRequired`,
//! `if`/`then`/`else`, etc.) directly from there. They are graduated to typed
//! fields under the J5–J8 beads (Phase 2b).

use serde::de::{Deserializer, Error as DeError, MapAccess, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Deref, DerefMut};

/// Map of `x-*` specification extensions. Any non-`x-`-prefixed key fails to
/// deserialize.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extensions(pub BTreeMap<String, Value>);

impl Extensions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, String, Value> {
        self.0.iter()
    }
}

impl Deref for Extensions {
    type Target = BTreeMap<String, Value>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Extensions {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Serialize for Extensions {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Extensions {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ExtVisitor;

        impl<'de> Visitor<'de> for ExtVisitor {
            type Value = Extensions;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a map of OAS specification extensions (`x-*` keys)")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut out: BTreeMap<String, Value> = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value: Value = map.next_value()?;
                    if !key.starts_with("x-") {
                        return Err(A::Error::custom(format!(
                            "unknown field `{key}` (OpenAPI specification extensions must start with `x-`)"
                        )));
                    }
                    out.insert(key, value);
                }
                Ok(Extensions(out))
            }
        }

        d.deserialize_map(ExtVisitor)
    }
}
