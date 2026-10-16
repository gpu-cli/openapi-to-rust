//! Opt-in additional derives for generated models and HTTP parameter enums.

use crate::{CodeGenerator, GeneratorError, Result, SchemaAnalysis, analysis::SchemaType};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use syn::{Attribute, Item, Meta, Path, Token, parse::Parser, punctuated::Punctuated};

/// Additive derives, configured by `[generator.derives]`.
///
/// Empty defaults preserve existing output. Derives are macro paths, such as
/// `Ord` or `my_crate::MyDerive`; consumers provide custom macro dependencies.
/// Required trait implementations on fields and referenced models are the
/// consumer's responsibility, and are checked by Rust rather than inferred.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct DerivesConfig {
    /// Applies to closed/extensible string enums, including hoisted inline
    /// enums and HTTP parameter enums. Payload unions are not string enums.
    pub string_enums: Vec<String>,
    /// Additional derives keyed by the exact emitted Rust type name.
    /// Valid types omitted by operation selection or pruning are ignored.
    /// Unknown/ambiguous names and type aliases are rejected.
    pub types: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Copy)]
pub(crate) enum DeriveTarget {
    StringEnum,
    Model,
    Alias,
}

fn validation(message: impl Into<String>) -> GeneratorError {
    GeneratorError::ValidationError(message.into())
}

fn parse_path(value: &str) -> Result<Path> {
    Path::parse_mod_style
        .parse_str(value)
        .map_err(|error| validation(format!("Invalid derive macro path `{value}`: {error}")))
}

// Only equate known standard spellings. Custom macros with the same final
// identifier may come from different crates and must remain distinct.
fn identity(path: &Path) -> String {
    let name = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    match name.as_str() {
        "Debug" | "std::fmt::Debug" | "core::fmt::Debug" => "Debug",
        "Display" | "std::fmt::Display" | "core::fmt::Display" => "Display",
        "Clone" | "std::clone::Clone" | "core::clone::Clone" => "Clone",
        "Copy" | "std::marker::Copy" | "core::marker::Copy" => "Copy",
        "Default" | "std::default::Default" | "core::default::Default" => "Default",
        "PartialEq" | "std::cmp::PartialEq" | "core::cmp::PartialEq" => "PartialEq",
        "Eq" | "std::cmp::Eq" | "core::cmp::Eq" => "Eq",
        "PartialOrd" | "std::cmp::PartialOrd" | "core::cmp::PartialOrd" => "PartialOrd",
        "Ord" | "std::cmp::Ord" | "core::cmp::Ord" => "Ord",
        "Hash" | "std::hash::Hash" | "core::hash::Hash" => "Hash",
        "Serialize" | "serde::Serialize" => "Serialize",
        "Deserialize" | "serde::Deserialize" => "Deserialize",
        "specta::Type" => "specta::Type",
        _ if path.leading_colon.is_some() => return format!("::{name}"),
        _ => return name,
    }
    .to_string()
}

fn collect_derives(meta: &Meta, existing: &mut BTreeSet<String>) -> syn::Result<()> {
    if let Meta::List(list) = meta {
        if list.path.is_ident("derive") {
            let paths = list.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)?;
            existing.extend(paths.iter().map(identity));
        } else if list.path.is_ident("cfg_attr") {
            let nested = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
            // Respect existing conditional derives, particularly Specta's
            // feature gate, rather than adding an unconditional duplicate.
            for meta in nested.iter().skip(1) {
                collect_derives(meta, existing)?;
            }
        }
    }
    Ok(())
}

impl DerivesConfig {
    pub(crate) fn is_empty(&self) -> bool {
        self.string_enums.is_empty() && self.types.is_empty()
    }

    pub(crate) fn validate_paths(&self) -> Result<()> {
        for (scope, values) in std::iter::once(("string_enums".to_string(), &self.string_enums))
            .chain(
                self.types
                    .iter()
                    .map(|(name, values)| (format!("types.{name}"), values)),
            )
        {
            for value in values {
                parse_path(value)
                    .map_err(|error| validation(format!("generator.derives.{scope}: {error}")))?;
            }
        }
        Ok(())
    }

    pub(crate) fn apply(
        &self,
        file: &mut syn::File,
        targets: &BTreeMap<String, DeriveTarget>,
    ) -> Result<()> {
        if self.is_empty() {
            return Ok(());
        }
        // Inspect the existing AST so manual wire implementations stay intact.
        let mut manual: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for item in &file.items {
            if let Item::Impl(item) = item {
                if let (Some((_, trait_path, _)), syn::Type::Path(self_type)) =
                    (&item.trait_, item.self_ty.as_ref())
                {
                    if let Some(segment) = self_type.path.segments.last() {
                        manual
                            .entry(segment.ident.to_string())
                            .or_default()
                            .insert(identity(trait_path));
                    }
                }
            }
        }
        for item in &mut file.items {
            let (name, attrs): (String, &mut Vec<Attribute>) = match item {
                Item::Struct(item) => (item.ident.to_string(), &mut item.attrs),
                Item::Enum(item) => (item.ident.to_string(), &mut item.attrs),
                _ => continue,
            };
            let Some(target) = targets.get(&name) else {
                continue;
            };
            let requested = self.types.get(&name).into_iter().flatten();
            let grouped = self
                .string_enums
                .iter()
                .filter(|_| matches!(target, DeriveTarget::StringEnum));
            let mut existing = BTreeSet::new();
            for attr in attrs.iter() {
                collect_derives(&attr.meta, &mut existing).map_err(|error| {
                    GeneratorError::CodeGenError(format!(
                        "Cannot inspect derives on `{name}`: {error}"
                    ))
                })?;
            }
            let mut additions = Vec::new();
            for value in grouped.chain(requested) {
                let path = parse_path(value)?;
                let key = identity(&path);
                if manual
                    .get(&name)
                    .is_some_and(|traits| traits.contains(&key))
                {
                    return Err(validation(format!(
                        "generator.derives: cannot derive `{value}` on `{name}` because the generator already implements that trait manually"
                    )));
                }
                if existing.insert(key) {
                    additions.push(path);
                }
            }
            if !additions.is_empty() {
                attrs.push(syn::parse_quote!(#[derive(#(#additions),*)]));
            }
        }
        Ok(())
    }
}

impl CodeGenerator {
    pub(crate) fn model_derive_targets(
        &self,
        analysis: &SchemaAnalysis,
    ) -> BTreeMap<String, DeriveTarget> {
        analysis
            .schemas
            .values()
            .filter_map(|schema| {
                let target = match &schema.schema_type {
                    SchemaType::StringEnum { .. } | SchemaType::ExtensibleEnum { .. } => {
                        DeriveTarget::StringEnum
                    }
                    SchemaType::Object { .. }
                    | SchemaType::Composition { .. }
                    | SchemaType::Union { .. }
                    | SchemaType::DiscriminatedUnion { .. } => DeriveTarget::Model,
                    SchemaType::Reference { target } if *target == schema.name => return None,
                    _ => DeriveTarget::Alias,
                };
                Some((self.to_rust_type_name(&schema.name), target))
            })
            .collect()
    }

    pub(crate) fn parameter_derive_targets(
        &self,
        operations: &[&crate::analysis::OperationInfo],
    ) -> BTreeMap<String, DeriveTarget> {
        operations
            .iter()
            .flat_map(|operation| &operation.parameters)
            .filter(|parameter| parameter.enum_values.is_some())
            .map(|parameter| (parameter.rust_type.clone(), DeriveTarget::StringEnum))
            .collect()
    }

    pub(crate) fn validate_derives(&self, analysis: &SchemaAnalysis) -> Result<()> {
        let config = &self.config().derives;
        if config.is_empty() {
            return Ok(());
        }
        config.validate_paths()?;
        // Validate before pruning, allowing one config to be reused across
        // operation selections without turning omitted targets into typos.
        let models = self.model_derive_targets(analysis);
        let operations: Vec<_> = analysis.operations.values().collect();
        let parameters = self.parameter_derive_targets(&operations);
        for name in config.types.keys() {
            if analysis
                .schemas
                .values()
                .filter(|schema| self.to_rust_type_name(&schema.name) == *name)
                .count()
                > 1
            {
                return Err(validation(format!(
                    "generator.derives.types: `{name}` is ambiguous between multiple model schemas"
                )));
            }
            match (models.get(name), parameters.get(name)) {
                (Some(_), Some(_)) => {
                    return Err(validation(format!(
                        "generator.derives.types: `{name}` is ambiguous between a model and an HTTP parameter enum"
                    )));
                }
                (Some(DeriveTarget::Alias), _) => {
                    return Err(validation(format!(
                        "generator.derives.types: `{name}` is a type alias; derives require a struct or enum"
                    )));
                }
                (None, None) => {
                    return Err(validation(format!(
                        "generator.derives.types: unknown generated type `{name}`; use the emitted Rust type name"
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }
}
