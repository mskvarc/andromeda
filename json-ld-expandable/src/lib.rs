//! Derive macro for JSON-LD expandable types.
//!
//! This crate provides the `JsonLdExpandable` derive macro that generates
//! `to_expanded()` implementations for converting Rust structs to expanded
//! JSON-LD format suitable for compaction.
//!
//! # Prerequisites
//!
//! The generated code depends on [`serde_json`](https://docs.rs/serde_json) at
//! the call site.  Make sure your crate has `serde_json` as a dependency,
//! otherwise the generated `to_expanded()` method will fail to compile.
//!
//! # Example
//!
//! ```ignore
//! use json_ld_expandable::JsonLdExpandable;
//!
//! #[derive(JsonLdExpandable)]
//! #[jsonld(type = "https://uri.etsi.org/ngsi-ld/EntityTypeInfo")]
//! pub struct EntityTypeInfo {
//!     #[jsonld(id)]
//!     pub id: String,
//!
//!     #[jsonld(property = "https://uri.etsi.org/ngsi-ld/typeName")]
//!     pub type_name: String,
//!
//!     #[jsonld(property = "https://uri.etsi.org/ngsi-ld/entityCount")]
//!     pub entity_count: i64,
//!
//!     #[jsonld(property = "https://uri.etsi.org/ngsi-ld/attributeDetails", nested, vec)]
//!     pub attribute_details: Vec<AttributeDetail>,
//! }
//! ```
//!
//! # Trait
//!
//! The generated code implements the `JsonLdExpandable` trait. You must define
//! this trait in your crate (by default at `crate::jsonld::JsonLdExpandable`)
//! or point to a custom path with `#[jsonld(crate = "...")]`:
//!
//! ```ignore
//! pub trait JsonLdExpandable {
//!     fn to_expanded(&self) -> serde_json::Value;
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input, spanned::Spanned};

/// Derive macro for generating `to_expanded()` method for JSON-LD types.
///
/// # Attributes
///
/// ## Container attributes (`#[jsonld(...)]` on struct)
///
/// - `type = "IRI"` - The JSON-LD @type IRI for this struct (required)
/// - `crate = "path"` - Custom path to the crate containing JsonLdExpandable trait
///   (defaults to `crate::jsonld`)
///
/// ## Field attributes (`#[jsonld(...)]` on fields)
///
/// - `id` - This field is the @id of the JSON-LD node
/// - `skip` - Skip this field in the expanded output
/// - `property = "IRI"` - The expanded IRI for this property
/// - `nested` - This field contains nested JsonLdExpandable types
/// - `vec` - This field is a Vec of items (use with `nested`)
/// - `list` - Wrap the value in a @list container
/// - `vocab` - Single vocab term (uses @id instead of @value)
/// - `vocab_vec` - Array of vocab terms (generates array of @id objects)
#[proc_macro_derive(JsonLdExpandable, attributes(jsonld))]
pub fn derive_json_ld_expandable(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match derive_impl(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn derive_impl(input: DeriveInput) -> Result<proc_macro2::TokenStream, syn::Error> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    // Extract container-level attributes
    let container_attrs = extract_container_attrs(&input.attrs)?;
    let type_iri = container_attrs
        .type_iri
        .ok_or_else(|| syn::Error::new_spanned(&input.ident, "JsonLdExpandable requires #[jsonld(type = \"...\")] attribute"))?;

    // Use custom crate path or default to crate::jsonld
    let trait_path: syn::Path = match container_attrs.crate_path {
        Some(p) => syn::parse_str(&p)?,
        None => syn::parse_str("crate::jsonld").unwrap(),
    };

    // Process fields
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input.ident,
                    "JsonLdExpandable only supports structs with named fields",
                ));
            }
        },
        _ => return Err(syn::Error::new_spanned(&input.ident, "JsonLdExpandable only supports structs")),
    };

    let mut id_field: Option<proc_macro2::TokenStream> = None;
    let mut property_fields: Vec<proc_macro2::TokenStream> = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let attrs = parse_field_attrs(&field.attrs)?;

        if attrs.skip {
            continue;
        }

        if attrs.is_id {
            id_field = Some(quote! {
                obj.insert("@id".to_string(), serde_json::json!(self.#field_name));
            });
            continue;
        }

        if let Some(property_iri) = &attrs.property {
            let is_option = is_option_type(&field.ty);

            // Build value expression for both direct (`self.field`) and unwrapped (`__val`) access.
            // When the field is Option<_>, we use `__val` inside an `if let Some` guard.
            let value_expr = if attrs.vocab {
                if is_option {
                    quote! { vec![serde_json::json!({"@id": __val})] }
                } else {
                    quote! { vec![serde_json::json!({"@id": self.#field_name})] }
                }
            } else if attrs.vocab_vec {
                if is_option {
                    quote! {
                        __val.iter()
                            .map(|item| serde_json::json!({"@id": item}))
                            .collect::<Vec<_>>()
                    }
                } else {
                    quote! {
                        self.#field_name.iter()
                            .map(|item| serde_json::json!({"@id": item}))
                            .collect::<Vec<_>>()
                    }
                }
            } else if attrs.nested {
                if attrs.is_vec {
                    quote! {
                        self.#field_name.iter()
                            .map(|item| #trait_path::JsonLdExpandable::to_expanded(item))
                            .collect::<Vec<_>>()
                    }
                } else {
                    quote! {
                        vec![#trait_path::JsonLdExpandable::to_expanded(&self.#field_name)]
                    }
                }
            } else if attrs.list {
                if is_option {
                    quote! { vec![serde_json::json!({"@list": __val})] }
                } else {
                    quote! { vec![serde_json::json!({"@list": self.#field_name})] }
                }
            } else {
                if is_option {
                    quote! { vec![serde_json::json!({"@value": __val})] }
                } else {
                    quote! { vec![serde_json::json!({"@value": self.#field_name})] }
                }
            };

            if is_option {
                property_fields.push(quote! {
                    if let Some(ref __val) = self.#field_name {
                        obj.insert(#property_iri.to_string(), serde_json::json!(#value_expr));
                    }
                });
            } else {
                property_fields.push(quote! {
                    obj.insert(#property_iri.to_string(), serde_json::json!(#value_expr));
                });
            }
        }
    }

    let id_insert = id_field.unwrap_or_else(|| quote! {});

    Ok(quote! {
        impl #impl_generics #trait_path::JsonLdExpandable for #name #ty_generics #where_clause {
            fn to_expanded(&self) -> serde_json::Value {
                let mut obj = serde_json::Map::new();

                #id_insert

                obj.insert("@type".to_string(), serde_json::json!([#type_iri]));

                #(#property_fields)*

                serde_json::Value::Object(obj)
            }
        }
    })
}

// ---------------------------------------------------------------------------
// IRI validation
// ---------------------------------------------------------------------------

/// Check whether a type is `Option<_>`.
fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident == "Option";
    }
    false
}

/// Check whether a string looks like a valid IRI (has a scheme component).
///
/// Accepts any string matching `[a-zA-Z][a-zA-Z0-9+.-]*:` at the start,
/// which covers `https:`, `http:`, `urn:`, and other standard schemes.
fn looks_like_iri(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !c.is_ascii_alphanumeric() && c != '+' && c != '-' && c != '.' {
            return false;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Container attributes
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ContainerAttrs {
    type_iri: Option<String>,
    crate_path: Option<String>,
}

/// Extract container-level attributes, returning errors on malformed input.
fn extract_container_attrs(attrs: &[syn::Attribute]) -> Result<ContainerAttrs, syn::Error> {
    let mut result = ContainerAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("jsonld") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("type") {
                let value: syn::LitStr = meta.value()?.parse()?;
                let iri = value.value();
                if !looks_like_iri(&iri) {
                    return Err(syn::Error::new(
                        value.span(),
                        format!(
                            "invalid IRI \"{}\": expected a full IRI with a scheme \
                             (e.g. \"https://example.com/Type\" or \"urn:example:Type\")",
                            iri
                        ),
                    ));
                }
                result.type_iri = Some(iri);
            } else if meta.path.is_ident("crate") {
                let value: syn::LitStr = meta.value()?.parse()?;
                result.crate_path = Some(value.value());
            } else {
                return Err(meta.error(format!(
                    "unknown jsonld container attribute `{}`; expected `type` or `crate`",
                    meta.path.get_ident().map_or("?".into(), |i| i.to_string())
                )));
            }
            Ok(())
        })?;
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Field attributes
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FieldAttrs {
    is_id: bool,
    skip: bool,
    property: Option<String>,
    nested: bool,
    list: bool,
    is_vec: bool,
    /// For `@type: @vocab` terms - uses `@id` instead of `@value`
    vocab: bool,
    /// For arrays of `@type: @vocab` terms - generates array of `@id` values
    vocab_vec: bool,
}

/// Parse field-level attributes, returning errors on malformed or conflicting input.
fn parse_field_attrs(attrs: &[syn::Attribute]) -> Result<FieldAttrs, syn::Error> {
    let mut result = FieldAttrs::default();
    let mut attr_span: Option<proc_macro2::Span> = None;

    for attr in attrs {
        if !attr.path().is_ident("jsonld") {
            continue;
        }

        attr_span = Some(attr.span());

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                result.is_id = true;
            } else if meta.path.is_ident("skip") {
                result.skip = true;
            } else if meta.path.is_ident("nested") {
                result.nested = true;
            } else if meta.path.is_ident("list") {
                result.list = true;
            } else if meta.path.is_ident("vec") {
                result.is_vec = true;
            } else if meta.path.is_ident("vocab") {
                result.vocab = true;
            } else if meta.path.is_ident("vocab_vec") {
                result.vocab_vec = true;
            } else if meta.path.is_ident("property") {
                let value: syn::LitStr = meta.value()?.parse()?;
                let iri = value.value();
                if !looks_like_iri(&iri) {
                    return Err(syn::Error::new(
                        value.span(),
                        format!(
                            "invalid property IRI \"{}\": expected a full IRI with a scheme \
                             (e.g. \"https://example.com/prop\" or \"urn:example:prop\")",
                            iri
                        ),
                    ));
                }
                result.property = Some(iri);
            } else {
                return Err(meta.error(format!(
                    "unknown jsonld field attribute `{}`; expected one of: \
                     id, skip, property, nested, vec, list, vocab, vocab_vec",
                    meta.path.get_ident().map_or("?".into(), |i| i.to_string())
                )));
            }
            Ok(())
        })?;
    }

    // Validate mutually exclusive / conflicting attributes
    let span = attr_span.unwrap_or_else(proc_macro2::Span::call_site);

    let value_modes: &[(&str, bool)] = &[
        ("nested", result.nested),
        ("list", result.list),
        ("vocab", result.vocab),
        ("vocab_vec", result.vocab_vec),
    ];
    let active: Vec<&str> = value_modes.iter().filter(|(_, set)| *set).map(|(name, _)| *name).collect();
    if active.len() > 1 {
        return Err(syn::Error::new(
            span,
            format!("conflicting jsonld attributes: {} are mutually exclusive", active.join(", ")),
        ));
    }

    if result.is_vec && !result.nested {
        return Err(syn::Error::new(span, "jsonld attribute `vec` can only be used together with `nested`"));
    }

    if result.is_id && (result.nested || result.list || result.vocab || result.vocab_vec || result.property.is_some()) {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `id` cannot be combined with property, nested, list, vocab, or vocab_vec",
        ));
    }

    if result.skip && (result.is_id || result.nested || result.list || result.vocab || result.vocab_vec || result.property.is_some()) {
        return Err(syn::Error::new(span, "jsonld attribute `skip` cannot be combined with other attributes"));
    }

    Ok(result)
}
