//! Guard against unvalidated IRI and media-type string constants.

use crate::source_scan::{is_test_only, workspace_sources};
use quote::ToTokens;
use std::path::Path;
use syn::{Attribute, Expr, ExprLit, ImplItem, Item, Lit, TraitItem, Type};

/// Assert that production constants do not encode IRIs or media types as `&str`.
///
/// Embedded documents are accepted because their contents are payload text,
/// while `iri!` and `MediaType` declarations are accepted by construction.
///
/// # Panics
///
/// Panics when a production source contains a raw IRI or media-type constant,
/// or when the workspace source tree cannot be read or parsed.
pub fn assert_no_raw_iri_or_media_constants(workspace_root: &str) {
    let violations = raw_iri_or_media_constants(workspace_root);
    assert!(violations.is_empty(), "raw IRI/media constants:\n{}", violations.join("\n"));
}

/// Return exact declarations that encode an IRI or media type as `&str`.
pub fn raw_iri_or_media_constants(workspace_root: &str) -> Vec<String> {
    let mut violations = Vec::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        scan_items(&source.workspace_path, &source.syntax.items, &mut violations);
    }
    violations.sort();
    violations
}

/// Inspect one top-level constant or static declaration.
#[cfg(test)]
fn raw_constant(path: &str, item: Item) -> Option<String> {
    let (attributes, identifier, ty, expression) = match item {
        Item::Const(item) => (item.attrs, item.ident, item.ty, item.expr),
        Item::Static(item) => (item.attrs, item.ident, item.ty, item.expr),
        Item::Enum(_)
        | Item::ExternCrate(_)
        | Item::Fn(_)
        | Item::ForeignMod(_)
        | Item::Impl(_)
        | Item::Macro(_)
        | Item::Mod(_)
        | Item::Struct(_)
        | Item::Trait(_)
        | Item::TraitAlias(_)
        | Item::Type(_)
        | Item::Union(_)
        | Item::Use(_)
        | Item::Verbatim(_) => return None,
        _ => return None,
    };
    raw_declaration(path, &identifier.to_string(), &attributes, &ty, &expression)
}

/// Recursively scan items, including associated constants.
fn scan_items(path: &str, items: &[Item], violations: &mut Vec<String>) {
    items.iter().for_each(|item| match item {
        Item::Const(item) if !is_test_only(&item.attrs) => {
            if let Some(violation) = raw_declaration(path, &item.ident.to_string(), &item.attrs, &item.ty, &item.expr) {
                violations.push(violation);
            }
        }
        Item::Impl(item) if !is_test_only(&item.attrs) => item.items.iter().for_each(|member| {
            if let ImplItem::Const(member) = member
                && let Some(violation) = raw_declaration(
                    path,
                    &format!("{}::{}", item.self_ty.to_token_stream(), member.ident),
                    &member.attrs,
                    &member.ty,
                    &member.expr,
                )
            {
                violations.push(violation);
            }
        }),
        Item::Mod(item) if !is_test_only(&item.attrs) => {
            if let Some((_, nested)) = &item.content {
                scan_items(path, nested, violations);
            }
        }
        Item::Static(item) if !is_test_only(&item.attrs) => {
            if let Some(violation) = raw_declaration(path, &item.ident.to_string(), &item.attrs, &item.ty, &item.expr) {
                violations.push(violation);
            }
        }
        Item::Trait(item) if !is_test_only(&item.attrs) => item.items.iter().for_each(|member| {
            if let TraitItem::Const(member) = member
                && let Some((_, default)) = &member.default
                && let Some(violation) = raw_declaration(path, &format!("{}::{}", item.ident, member.ident), &member.attrs, &member.ty, default)
            {
                violations.push(violation);
            }
        }),
        Item::Const(_)
        | Item::Enum(_)
        | Item::ExternCrate(_)
        | Item::Fn(_)
        | Item::ForeignMod(_)
        | Item::Impl(_)
        | Item::Macro(_)
        | Item::Mod(_)
        | Item::Static(_)
        | Item::Struct(_)
        | Item::Trait(_)
        | Item::TraitAlias(_)
        | Item::Type(_)
        | Item::Union(_)
        | Item::Use(_)
        | Item::Verbatim(_) => {}
        _ => {}
    });
}

/// Inspect one constant declaration shape.
fn raw_declaration(path: &str, identifier: &str, attributes: &[Attribute], ty: &Type, expression: &Expr) -> Option<String> {
    if is_test_only(attributes) || !is_str_reference(ty) {
        return None;
    }
    let Expr::Lit(ExprLit { lit: Lit::Str(value), .. }) = expression else {
        return None;
    };
    let value = value.value();
    (looks_like_iri(&value) || looks_like_media_type(&value)).then(|| format!("{path}::{identifier}: {} = {:?}", ty.to_token_stream(), value))
}

/// Whether a type is exactly a shared string slice.
fn is_str_reference(ty: &Type) -> bool {
    matches!(ty, Type::Reference(reference) if matches!(reference.elem.as_ref(), Type::Path(path) if path.path.is_ident("str")))
}

/// Whether text has an absolute IRI scheme used by this workspace.
fn looks_like_iri(value: &str) -> bool {
    ["http://", "https://", "urn:"].iter().any(|prefix| value.starts_with(prefix))
}

/// Whether text is one or more Internet media types.
fn looks_like_media_type(value: &str) -> bool {
    value.split(',').all(|part| {
        let part = part.trim();
        [
            "application/",
            "audio/",
            "font/",
            "image/",
            "message/",
            "model/",
            "multipart/",
            "text/",
            "video/",
        ]
        .iter()
        .any(|prefix| part.starts_with(prefix))
    })
}

#[cfg(test)]
mod tests {
    use crate::raw_constant::{raw_constant, scan_items};
    use syn::Item;

    /// Parse one fixture declaration.
    fn fixture(source: &str) -> Item {
        syn::parse_str(source).unwrap_or_else(|error| unreachable!("fixture parses: {error}"))
    }

    #[test]
    fn rejects_raw_iri_and_media_type_constants() {
        assert!(raw_constant("fixture.rs", fixture("const IRI: &str = \"https://example.com/id\";")).is_some());
        assert!(raw_constant("fixture.rs", fixture("const MEDIA: &str = \"application/ld+json, application/json\";")).is_some());
        let syntax = syn::parse_file("impl Relation { const IRI: &str = \"https://example.com/relation\"; }")
            .unwrap_or_else(|error| unreachable!("fixture parses: {error}"));
        let mut violations = Vec::new();
        scan_items("fixture.rs", &syntax.items, &mut violations);
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn accepts_typed_constants_and_embedded_documents() {
        assert!(raw_constant("fixture.rs", fixture("const IRI: Iri<&str> = iri!(\"https://example.com/id\");")).is_none());
        assert!(raw_constant("fixture.rs", fixture("const MEDIA: MediaType = APPLICATION_JSON;")).is_none());
        assert!(raw_constant("fixture.rs", fixture("const DOCUMENT: &str = include_str!(\"context.jsonld\");")).is_none());
    }
}
