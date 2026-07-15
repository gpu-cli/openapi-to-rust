//! Specification Extensions support — `x-*` fields per OAS §"Specification Extensions".
//!
//! `Extensions` is a compatibility-oriented flatten target. It retains `x-*`
//! specification extensions and other leftover keys so imperfect real-world
//! documents continue to parse. Callers can inspect [`Extensions::non_extension_keys`]
//! when they want to diagnose fields outside the OAS extension convention.
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
//! `Schema` and `SchemaDetails` also retain a loose `extra` map. Their common
//! JSON Schema 2020-12 keywords are typed, while the open JSON Schema
//! vocabulary still requires compatibility storage for unknown keywords.

use serde::de::{Deserializer, MapAccess, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Deref, DerefMut};

/// Map of specification extensions and other compatibility-preserved fields.
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
                f.write_str("a map of extension and compatibility fields")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                // We accept any leftover keys here so real-world specs that
                // sprinkle non-`x-` fields in places they don't belong (we've
                // observed `produces`, `in`, `type`, `density`, `title`,
                // `description` on the wrong objects) still parse. Consumers
                // can inspect non-`x-` keys via `non_extension_keys`.
                let mut out: BTreeMap<String, Value> = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value: Value = map.next_value()?;
                    out.insert(key, value);
                }
                Ok(Extensions(out))
            }
        }

        d.deserialize_map(ExtVisitor)
    }
}

impl Extensions {
    /// Iterate keys that don't follow the OAS `x-*` extension convention.
    /// These are typically OAS 2.0 leftovers (`produces`/`consumes`) or
    /// fields placed on the wrong object level. They are retained rather than
    /// rejected at deserialize time.
    pub fn non_extension_keys(&self) -> impl Iterator<Item = &str> {
        self.0
            .keys()
            .filter(|k| !k.starts_with("x-"))
            .map(String::as_str)
    }
}
