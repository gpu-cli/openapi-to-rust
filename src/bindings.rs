//! Versioned metadata captured from the AST used to emit Rust bindings.
//!
//! Paths are relative to the generated module mount point. Rust token strings
//! preserve the emitted syntax, including generics, bounds, and Option/Box
//! nesting. Attributes describe conditional compilation and serde behavior.

use crate::{GenerationResult, GeneratorError, Result};
use quote::ToTokens;
use serde::{Deserialize, Serialize};
use syn::{Item, Visibility};

/// An additive library result; existing `GenerationResult` literals remain valid.
#[derive(Debug, Clone)]
pub struct GenerationWithBindings {
    pub generation: GenerationResult,
    pub bindings: BindingsMetadata,
}

/// Version 1 covers models and the HTTP client, including enabled builders and
/// public helpers. Dedicated configured streaming, server, and registry output
/// are rejected rather than silently omitted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingsMetadata {
    pub schema_version: u32,
    pub generator_version: String,
    pub module_label: String,
    pub coverage: Vec<String>,
    pub modules: Vec<String>,
    pub reexports: Vec<String>,
    pub symbols: Vec<BindingSymbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingSymbol {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub attributes: Vec<String>,
    pub generics: String,
    pub impl_generics: Option<String>,
    pub impl_trait: Option<String>,
    pub rust_type: Option<String>,
    pub fields: Vec<BindingField>,
    pub variants: Vec<BindingVariant>,
    pub signature: Option<BindingSignature>,
    pub operation: Option<BindingOperation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingField {
    pub name: Option<String>,
    pub index: usize,
    pub public: bool,
    pub wire_name: Option<String>,
    pub rust_type: String,
    pub attributes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingVariant {
    pub name: String,
    pub wire_name: Option<String>,
    pub attributes: Vec<String>,
    pub fields: Vec<BindingField>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingSignature {
    /// Canonical token spelling of the signature shared with Rust emission.
    pub rust: String,
    pub asynchronous: bool,
    pub receiver: Option<String>,
    pub arguments: Vec<BindingArgument>,
    pub return_type: String,
    pub generics: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingArgument {
    pub name: String,
    pub rust_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BindingOperation {
    pub operation_id: String,
    pub source_json_pointer: String,
    pub source_method: String,
    pub source_path: String,
    pub source_operation_id: Option<String>,
    pub webhook: bool,
    pub response_statuses: Vec<String>,
    /// Exact statuses excluded from an OpenAPI status-class representation.
    pub response_excluded_statuses: Vec<String>,
    pub response_media_type: Option<String>,
    pub response_kind: String,
    pub consumption: String,
    pub multipart_filenames: bool,
}

impl BindingsMetadata {
    pub(crate) fn new(module_label: &str, http_client: bool) -> Self {
        let mut coverage = vec!["models".to_string(), "public_helpers".to_string()];
        if http_client {
            coverage.extend(["http_client".to_string(), "enabled_builders".to_string()]);
        }
        Self {
            schema_version: 1,
            generator_version: crate::VERSION.to_string(),
            module_label: module_label.to_string(),
            coverage,
            modules: Vec::new(),
            reexports: Vec::new(),
            symbols: Vec::new(),
        }
    }

    /// Stable JSON with no timestamps or machine-specific input/output paths.
    pub fn to_json(&self) -> Result<String> {
        let mut output = serde_json::to_string_pretty(self).map_err(|error| {
            GeneratorError::CodeGenError(format!("Failed to serialize binding metadata: {error}"))
        })?;
        output.push('\n');
        Ok(output)
    }

    pub(crate) fn finish(&mut self) {
        self.modules.sort();
        self.modules.dedup();
        self.reexports.sort();
        self.reexports.dedup();
        self.symbols.sort_by(|a, b| {
            (&a.path, &a.kind, &a.impl_trait, &a.attributes).cmp(&(
                &b.path,
                &b.kind,
                &b.impl_trait,
                &b.attributes,
            ))
        });
    }

    /// Inspect the emitter's already parsed AST before prettyplease formats it.
    pub(crate) fn collect(&mut self, module: &str, ast: &syn::File) {
        if module == "sse" {
            self.coverage.push("sse_runtime".to_string());
        }
        self.modules.push(module.to_string());
        self.collect_items(module, &ast.items);
    }

    fn collect_items(&mut self, module: &str, items: &[Item]) {
        let public_types: std::collections::BTreeSet<_> = items
            .iter()
            .filter_map(|item| match item {
                Item::Struct(item) if public(&item.vis) => Some(item.ident.to_string()),
                Item::Enum(item) if public(&item.vis) => Some(item.ident.to_string()),
                Item::Type(item) if public(&item.vis) => Some(item.ident.to_string()),
                _ => None,
            })
            .collect();
        for item in items {
            match item {
                Item::Struct(item) if public(&item.vis) => {
                    let mut symbol = symbol(
                        module,
                        &item.ident.to_string(),
                        "struct",
                        &item.attrs,
                        &item.generics,
                    );
                    symbol.fields = fields(
                        &item.fields,
                        serde_derived(&item.attrs),
                        serde_string(&item.attrs, "rename_all").as_deref(),
                    );
                    self.symbols.push(symbol);
                }
                Item::Enum(item) if public(&item.vis) => {
                    let mut symbol = symbol(
                        module,
                        &item.ident.to_string(),
                        "enum",
                        &item.attrs,
                        &item.generics,
                    );
                    symbol.variants = item
                        .variants
                        .iter()
                        .map(|variant| BindingVariant {
                            name: variant.ident.to_string(),
                            wire_name: wire_name(
                                &variant.attrs,
                                serde_derived(&item.attrs)
                                    .then(|| {
                                        renamed(
                                            &variant.ident.to_string(),
                                            serde_string(&item.attrs, "rename_all").as_deref(),
                                        )
                                    })
                                    .as_deref(),
                            ),
                            attributes: attributes(&variant.attrs),
                            fields: fields(
                                &variant.fields,
                                serde_derived(&item.attrs),
                                serde_string(&variant.attrs, "rename_all").as_deref(),
                            )
                            .into_iter()
                            .map(|mut field| {
                                field.public = true;
                                field
                            })
                            .collect(),
                        })
                        .collect();
                    self.symbols.push(symbol);
                }
                Item::Type(item) if public(&item.vis) => {
                    let mut symbol = symbol(
                        module,
                        &item.ident.to_string(),
                        "alias",
                        &item.attrs,
                        &item.generics,
                    );
                    symbol.rust_type = Some(tokens(&item.ty));
                    self.symbols.push(symbol);
                }
                Item::Fn(item) if public(&item.vis) => {
                    let mut symbol = symbol(
                        module,
                        &item.sig.ident.to_string(),
                        "function",
                        &item.attrs,
                        &item.sig.generics,
                    );
                    symbol.signature = Some(signature(&item.sig));
                    self.symbols.push(symbol);
                }
                Item::Impl(item) => {
                    let syn::Type::Path(owner) = item.self_ty.as_ref() else {
                        continue;
                    };
                    let Some(owner) = owner.path.segments.last() else {
                        continue;
                    };
                    if !public_types.contains(&owner.ident.to_string()) {
                        continue;
                    }
                    for method in &item.items {
                        match method {
                            syn::ImplItem::Fn(method) if public(&method.vis) => {
                                let owner_path = format!("{module}::{}", owner.ident);
                                let mut symbol = symbol(
                                    &owner_path,
                                    &method.sig.ident.to_string(),
                                    "method",
                                    &method.attrs,
                                    &method.sig.generics,
                                );
                                symbol.attributes.extend(attributes(&item.attrs));
                                symbol.impl_generics = Some(generics(&item.generics));
                                symbol.impl_trait =
                                    item.trait_.as_ref().map(|(_, path, _)| tokens(path));
                                symbol.signature = Some(signature(&method.sig));
                                self.symbols.push(symbol);
                            }
                            syn::ImplItem::Const(value) if public(&value.vis) => {
                                let owner_path = format!("{module}::{}", owner.ident);
                                let mut symbol = symbol(
                                    &owner_path,
                                    &value.ident.to_string(),
                                    "constant",
                                    &value.attrs,
                                    &syn::Generics::default(),
                                );
                                symbol.rust_type = Some(tokens(&value.ty));
                                symbol.impl_generics = Some(generics(&item.generics));
                                self.symbols.push(symbol);
                            }
                            _ => {}
                        }
                    }
                }
                Item::Mod(item) if public(&item.vis) => {
                    let path = format!("{module}::{}", item.ident);
                    self.modules.push(path.clone());
                    self.symbols.push(symbol(
                        module,
                        &item.ident.to_string(),
                        "module",
                        &item.attrs,
                        &syn::Generics::default(),
                    ));
                    if let Some((_, items)) = &item.content {
                        self.collect_items(&path, items);
                    }
                }
                Item::Const(item) if public(&item.vis) => {
                    let mut symbol = symbol(
                        module,
                        &item.ident.to_string(),
                        "constant",
                        &item.attrs,
                        &syn::Generics::default(),
                    );
                    symbol.rust_type = Some(tokens(&item.ty));
                    self.symbols.push(symbol);
                }
                Item::Use(item) if public(&item.vis) => {
                    self.reexports
                        .push(format!("{module}::{}", tokens(&item.tree)));
                }
                _ => {}
            }
        }
    }
}

fn public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}
fn tokens(value: &impl ToTokens) -> String {
    value.to_token_stream().to_string()
}
fn attributes(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attribute| !attribute.path().is_ident("doc"))
        .map(tokens)
        .collect()
}
fn generics(value: &syn::Generics) -> String {
    let params = tokens(value);
    match &value.where_clause {
        Some(clause) => format!("{params} {}", tokens(clause)).trim().to_string(),
        None => params,
    }
}
fn symbol(
    module: &str,
    name: &str,
    kind: &str,
    attrs: &[syn::Attribute],
    params: &syn::Generics,
) -> BindingSymbol {
    BindingSymbol {
        path: format!("{module}::{name}"),
        name: name.to_string(),
        kind: kind.to_string(),
        attributes: attributes(attrs),
        generics: generics(params),
        impl_generics: None,
        impl_trait: None,
        rust_type: None,
        fields: Vec::new(),
        variants: Vec::new(),
        signature: None,
        operation: None,
    }
}
fn fields(value: &syn::Fields, default_wire: bool, rename_all: Option<&str>) -> Vec<BindingField> {
    value
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = field.ident.as_ref().map(ToString::to_string);
            let fallback = name
                .as_deref()
                .filter(|_| default_wire)
                .map(|name| renamed(name, rename_all));
            BindingField {
                wire_name: wire_name(&field.attrs, fallback.as_deref()),
                name,
                index,
                public: public(&field.vis),
                rust_type: tokens(&field.ty),
                attributes: attributes(&field.attrs),
            }
        })
        .collect()
}
fn serde_derived(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().filter(|attribute| attribute.path().is_ident("derive")).any(|attribute| {
        attribute.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
            .is_ok_and(|paths| paths.iter().any(|path| path.segments.last().is_some_and(|segment| segment.ident == "Serialize" || segment.ident == "Deserialize")))
    }) && !attrs.iter().filter(|attribute| attribute.path().is_ident("serde")).any(|attribute| {
        attribute.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
            .is_ok_and(|parts| parts.iter().any(|part| matches!(part, syn::Meta::Path(path) if path.is_ident("untagged"))))
    })
}
fn serde_string(attrs: &[syn::Attribute], key: &str) -> Option<String> {
    for attribute in attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
    {
        if let Ok(parts) = attribute.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            for part in parts {
                if let syn::Meta::NameValue(value) = part {
                    if value.path.is_ident(key) {
                        if let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(value),
                            ..
                        }) = value.value
                        {
                            return Some(value.value());
                        }
                    }
                }
            }
        }
    }
    None
}
fn renamed(name: &str, rule: Option<&str>) -> String {
    use heck::{
        ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
        ToUpperCamelCase,
    };
    let name = name.strip_prefix("r#").unwrap_or(name);
    match rule {
        Some("lowercase") => name.to_lowercase(),
        Some("UPPERCASE") => name.to_uppercase(),
        Some("PascalCase") => name.to_upper_camel_case(),
        Some("camelCase") => name.to_lower_camel_case(),
        Some("snake_case") => name.to_snake_case(),
        Some("SCREAMING_SNAKE_CASE") => name.to_shouty_snake_case(),
        Some("kebab-case") => name.to_kebab_case(),
        Some("SCREAMING-KEBAB-CASE") => name.to_shouty_kebab_case(),
        _ => name.to_string(),
    }
}
fn wire_name(attrs: &[syn::Attribute], fallback: Option<&str>) -> Option<String> {
    for attribute in attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
    {
        let parsed = attribute.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        );
        if let Ok(parts) = parsed {
            for part in parts {
                match part {
                    syn::Meta::Path(path) if path.is_ident("flatten") || path.is_ident("skip") => {
                        return None;
                    }
                    syn::Meta::NameValue(value) if value.path.is_ident("rename") => {
                        if let syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(value),
                            ..
                        }) = value.value
                        {
                            return Some(value.value());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    fallback.map(|name| name.strip_prefix("r#").unwrap_or(name).to_string())
}
fn signature(value: &syn::Signature) -> BindingSignature {
    // Optional trailing punctuation changes with prettyplease line wrapping;
    // canonical signatures retain the same AST semantics on either side.
    let mut canonical = value.clone();
    if canonical.inputs.trailing_punct() {
        canonical.inputs.pop_punct();
    }
    if canonical.generics.params.trailing_punct() {
        canonical.generics.params.pop_punct();
    }
    let mut receiver = None;
    let mut arguments = Vec::new();
    for argument in &value.inputs {
        match argument {
            syn::FnArg::Receiver(value) => receiver = Some(tokens(value)),
            syn::FnArg::Typed(value) => arguments.push(BindingArgument {
                name: tokens(&value.pat),
                rust_type: tokens(&value.ty),
            }),
        }
    }
    let return_type = match &value.output {
        syn::ReturnType::Default => "()".to_string(),
        syn::ReturnType::Type(_, ty) => tokens(ty),
    };
    BindingSignature {
        rust: tokens(&canonical),
        asynchronous: value.asyncness.is_some(),
        receiver,
        arguments,
        return_type,
        generics: generics(&value.generics),
    }
}
