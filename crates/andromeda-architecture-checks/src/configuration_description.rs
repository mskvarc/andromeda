//! Every table, key and choice of the configuration carries an operator
//! description of its own.
//!
//! The JSON Schema, the template and the documentation's key reference all
//! read `#[schemars(description = "...")]`. Where it is missing the schema
//! falls back to the rustdoc, which is written for the code and carries
//! intra-doc links, so every type of the configuration crate deriving
//! `JsonSchema` must give one on itself, on each named field and on each
//! variant.

use crate::source_scan::{is_test_only, workspace_sources};
use proc_macro2::Group;
use std::path::Path;
use syn::{Attribute, Expr, Fields, Item, ItemEnum, ItemStruct, Token, token::Paren};

/// The crate whose types the rule covers.
const CONFIGURATION_SOURCES: &str = "crates/andromeda-configuration/src/";

/// Every type, field and variant of the configuration crate that reaches the
/// schema without an operator description, as `path: name`.
#[must_use]
pub fn undescribed_configuration_items(workspace_root: &str) -> Vec<String> {
    workspace_sources(Path::new(workspace_root))
        .into_iter()
        .filter(|source| source.workspace_path.starts_with(CONFIGURATION_SOURCES) && !source.workspace_path.ends_with("tests.rs"))
        .flat_map(|source| {
            source_violations(&source.syntax)
                .into_iter()
                .map(move |name| format!("{}: {name}", source.workspace_path))
        })
        .collect()
}

/// Assert that every type, field and variant of the configuration that
/// reaches the schema carries an operator description.
///
/// # Panics
///
/// Panics on any item [`undescribed_configuration_items`] reports, or when the
/// workspace source tree cannot be read or parsed.
pub fn assert_configuration_is_described(workspace_root: &str) {
    let missing = undescribed_configuration_items(workspace_root);
    assert!(
        missing.is_empty(),
        "every configuration type, key and choice carries #[schemars(description = \"...\")], written for the operator:\n{}",
        missing.join("\n")
    );
}

/// The undescribed items of one source, in source order.
fn source_violations(syntax: &syn::File) -> Vec<String> {
    syntax.items.iter().flat_map(item_violations).collect()
}

/// The undescribed parts of one item, and of the items of an inline module.
fn item_violations(item: &Item) -> Vec<String> {
    if let Item::Struct(item) = item
        && derives_json_schema(&item.attrs)
        && !is_test_only(&item.attrs)
    {
        return struct_violations(item);
    }
    if let Item::Enum(item) = item
        && derives_json_schema(&item.attrs)
        && !is_test_only(&item.attrs)
    {
        return enum_violations(item);
    }
    if let Item::Mod(module) = item
        && !is_test_only(&module.attrs)
    {
        return module.content.iter().flat_map(|(_, items)| items.iter().flat_map(item_violations)).collect();
    }
    Vec::new()
}

/// The struct and each of its named fields the schema reads.
fn struct_violations(item: &ItemStruct) -> Vec<String> {
    let name = item.ident.to_string();
    let fields = match &item.fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .filter(|field| !skipped(&field.attrs) && !described(&field.attrs))
            .filter_map(|field| field.ident.as_ref().map(|field| format!("{name}.{field}")))
            .collect(),
        Fields::Unnamed(_) | Fields::Unit => Vec::new(),
    };
    undescribed(&item.attrs, &name).into_iter().chain(fields).collect()
}

/// The enum and each of its variants.
fn enum_violations(item: &ItemEnum) -> Vec<String> {
    let name = item.ident.to_string();
    let variants = item
        .variants
        .iter()
        .filter(|variant| !skipped(&variant.attrs) && !described(&variant.attrs))
        .map(|variant| format!("{name}::{}", variant.ident));
    undescribed(&item.attrs, &name).into_iter().chain(variants).collect()
}

/// `name` when `attributes` carry no description.
fn undescribed(attributes: &[Attribute], name: &str) -> Option<String> {
    (!described(attributes)).then(|| name.to_owned())
}

/// Whether `attributes` derive `JsonSchema`, by its bare or its path name.
fn derives_json_schema(attributes: &[Attribute]) -> bool {
    attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("derive"))
        .any(|attribute| nested_names(attribute).iter().any(|name| name == "JsonSchema"))
}

/// Whether `attributes` give a `#[schemars(description = ...)]`.
fn described(attributes: &[Attribute]) -> bool {
    attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("schemars"))
        .any(|attribute| nested_names(attribute).iter().any(|name| name == "description"))
}

/// Whether `attributes` keep the item out of the file with `#[serde(skip)]`.
fn skipped(attributes: &[Attribute]) -> bool {
    attributes
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
        .any(|attribute| nested_names(attribute).iter().any(|name| name == "skip"))
}

/// The last segment of every name an attribute's list holds, its arguments
/// read past.
fn nested_names(attribute: &Attribute) -> Vec<String> {
    let mut names = Vec::new();
    // A list that does not parse names nothing this rule reads.
    let _parsed = attribute.parse_nested_meta(|meta| {
        names.extend(meta.path.segments.last().map(|segment| segment.ident.to_string()));
        if meta.input.peek(Token![=]) {
            let _value: Expr = meta.value()?.parse()?;
        } else if meta.input.peek(Paren) {
            let _arguments: Group = meta.input.parse()?;
        }
        Ok(())
    });
    names
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::configuration_description::source_violations;
    use pretty_assertions::assert_eq;

    fn violations(source: &str) -> Vec<String> {
        source_violations(&syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}")))
    }

    #[test]
    fn a_described_table_key_and_choice_pass() {
        let source = r#"
            #[derive(Deserialize, JsonSchema)]
            #[schemars(description = "Where the broker listens.")]
            struct ServerConfig {
                #[schemars(description = "The port.")]
                port: u16,
                #[serde(skip)]
                cache: u8,
            }
            #[derive(JsonSchema)]
            #[schemars(with = "String", description = "A mode.")]
            enum Mode {
                #[schemars(description = "Off.")]
                Disabled,
            }
        "#;
        assert_eq!(violations(source), [] as [String; 0]);
    }

    #[test]
    fn an_undescribed_table_key_and_choice_are_reported() {
        let source = r#"
            #[derive(schemars::JsonSchema)]
            struct ServerConfig {
                /// Rustdoc is not an operator description.
                port: u16,
            }
            #[derive(JsonSchema)]
            #[schemars(description = "A mode.")]
            enum Mode {
                #[schemars(rename = "off")]
                Disabled,
            }
        "#;
        assert_eq!(violations(source), ["ServerConfig", "ServerConfig.port", "Mode::Disabled"]);
    }

    #[test]
    fn types_without_the_derive_and_test_types_are_free() {
        let source = r"
            struct Plain { port: u16 }
            #[cfg(test)]
            mod tests {
                #[derive(JsonSchema)]
                struct Probe { port: u16 }
            }
        ";
        assert_eq!(violations(source), [] as [String; 0]);
    }
}
