//! Derive macro for JSON-LD expandable types.
//!
//! This crate provides the `JsonLdExpandable` derive macro that generates
//! `to_expanded()` implementations for converting Rust structs and enums to
//! expanded JSON-LD format suitable for compaction.
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
/// - `type = "IRI"` - The JSON-LD @type IRI for this struct (required for structs, unless `type_field` is used)
/// - `type_field` - Indicates @type is provided by a field marked with `type_value` (mutually exclusive with `type`)
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
/// - `typed_value` - Emit `{"@value": val, "@type": datatype}` (requires `datatype`)
/// - `datatype = "IRI"` - The @type IRI for typed values (requires `typed_value`)
/// - `json_value` - Emit `{"@value": val, "@type": "@json"}`
/// - `id_ref` - Emit `{"@id": val}` for URI references (can combine with `list`)
/// - `language_map` - Expand via `JsonLdLanguageMap::to_expanded_language_map()`
/// - `flatten_map` - Expand map entries as separate IRI-keyed properties
/// - `custom` - Use field's `JsonLdExpandable::to_expanded()` directly
/// - `type_value` - This field provides the dynamic @type (requires `type_field` on container)
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

    // Use custom crate path or default to crate::jsonld
    let trait_path: syn::Path = match container_attrs.crate_path {
        Some(ref p) => syn::parse_str(p)?,
        None => syn::parse_str("crate::jsonld").unwrap(),
    };

    match &input.data {
        Data::Struct(data) => derive_struct(name, &impl_generics, &ty_generics, where_clause, &trait_path, &container_attrs, &data.fields),
        Data::Enum(data) => derive_enum(name, &impl_generics, &ty_generics, where_clause, &trait_path, &container_attrs, &data.variants),
        Data::Union(_) => Err(syn::Error::new_spanned(&input.ident, "JsonLdExpandable does not support unions")),
    }
}

/// Generate `JsonLdExpandable` impl for a struct.
fn derive_struct(
    name: &syn::Ident,
    impl_generics: &syn::ImplGenerics<'_>,
    ty_generics: &syn::TypeGenerics<'_>,
    where_clause: Option<&syn::WhereClause>,
    trait_path: &syn::Path,
    container_attrs: &ContainerAttrs,
    fields: &Fields,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let named_fields = match fields {
        Fields::Named(fields) => &fields.named,
        _ => {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "JsonLdExpandable only supports structs with named fields",
            ));
        }
    };

    // Validate container attrs: exactly one of `type` or `type_field` required
    if container_attrs.type_iri.is_some() && container_attrs.type_field {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "jsonld attributes `type` and `type_field` are mutually exclusive",
        ));
    }
    if container_attrs.type_iri.is_none() && !container_attrs.type_field {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "JsonLdExpandable requires either #[jsonld(type = \"...\")] or #[jsonld(type_field)]",
        ));
    }

    let mut id_field: Option<proc_macro2::TokenStream> = None;
    let mut type_insert: Option<proc_macro2::TokenStream> = None;
    let mut property_fields: Vec<proc_macro2::TokenStream> = Vec::new();

    // If static type, generate the @type insert
    if let Some(ref type_iri) = container_attrs.type_iri {
        type_insert = Some(quote! {
            obj.insert("@type".to_string(), serde_json::json!([#type_iri]));
        });
    }

    for field in named_fields {
        let field_name = field.ident.as_ref().unwrap();
        let attrs = parse_field_attrs(&field.attrs)?;

        if attrs.skip {
            continue;
        }

        // Validate type_value field
        if attrs.type_value {
            if !container_attrs.type_field {
                return Err(syn::Error::new(
                    field.span(),
                    "jsonld attribute `type_value` requires `type_field` on the container",
                ));
            }
            type_insert = Some(quote! {
                obj.insert("@type".to_string(), #trait_path::JsonLdTypeValue::to_type_array(&self.#field_name));
            });
            continue;
        }

        if attrs.is_id {
            id_field = Some(quote! {
                obj.insert("@id".to_string(), serde_json::json!(self.#field_name));
            });
            continue;
        }

        // flatten_map: no property IRI, keys come from the map
        if attrs.flatten_map {
            property_fields.push(quote! {
                for (__key, __val) in &self.#field_name {
                    obj.insert(__key.to_string(), #trait_path::JsonLdExpandable::to_expanded(__val));
                }
            });
            continue;
        }

        if let Some(property_iri) = &attrs.property {
            let is_option = is_option_type(&field.ty);

            let value_expr = if attrs.typed_value {
                let datatype = attrs.datatype.as_ref().unwrap();
                if is_option {
                    quote! { serde_json::json!([{"@value": __val, "@type": #datatype}]) }
                } else {
                    quote! { serde_json::json!([{"@value": self.#field_name, "@type": #datatype}]) }
                }
            } else if attrs.json_value {
                if is_option {
                    quote! { serde_json::json!([{"@value": __val, "@type": "@json"}]) }
                } else {
                    quote! { serde_json::json!([{"@value": self.#field_name, "@type": "@json"}]) }
                }
            } else if attrs.id_ref {
                if attrs.list {
                    // list + id_ref combination
                    if is_option {
                        quote! {
                            serde_json::json!([{"@list": __val.iter().map(|__item| serde_json::json!({"@id": __item.to_string()})).collect::<Vec<_>>()}])
                        }
                    } else {
                        quote! {
                            serde_json::json!([{"@list": self.#field_name.iter().map(|__item| serde_json::json!({"@id": __item.to_string()})).collect::<Vec<_>>()}])
                        }
                    }
                } else {
                    // plain id_ref
                    if is_option {
                        quote! { serde_json::json!([{"@id": __val.to_string()}]) }
                    } else {
                        quote! { serde_json::json!([{"@id": self.#field_name.to_string()}]) }
                    }
                }
            } else if attrs.language_map {
                if is_option {
                    quote! { #trait_path::JsonLdLanguageMap::to_expanded_language_map(__val) }
                } else {
                    quote! { #trait_path::JsonLdLanguageMap::to_expanded_language_map(&self.#field_name) }
                }
            } else if attrs.custom {
                if is_option {
                    quote! { #trait_path::JsonLdExpandable::to_expanded(__val) }
                } else {
                    quote! { #trait_path::JsonLdExpandable::to_expanded(&self.#field_name) }
                }
            } else if attrs.vocab {
                if is_option {
                    quote! { serde_json::json!([{"@id": __val}]) }
                } else {
                    quote! { serde_json::json!([{"@id": self.#field_name}]) }
                }
            } else if attrs.vocab_vec {
                if is_option {
                    quote! {
                        serde_json::json!(__val.iter()
                            .map(|item| serde_json::json!({"@id": item}))
                            .collect::<Vec<_>>())
                    }
                } else {
                    quote! {
                        serde_json::json!(self.#field_name.iter()
                            .map(|item| serde_json::json!({"@id": item}))
                            .collect::<Vec<_>>())
                    }
                }
            } else if attrs.nested {
                if attrs.is_vec {
                    quote! {
                        serde_json::json!(self.#field_name.iter()
                            .map(|item| #trait_path::JsonLdExpandable::to_expanded(item))
                            .collect::<Vec<_>>())
                    }
                } else {
                    quote! {
                        serde_json::json!([#trait_path::JsonLdExpandable::to_expanded(&self.#field_name)])
                    }
                }
            } else if attrs.list {
                if is_option {
                    quote! { serde_json::json!([{"@list": __val}]) }
                } else {
                    quote! { serde_json::json!([{"@list": self.#field_name}]) }
                }
            } else {
                // default: plain @value
                if is_option {
                    quote! { serde_json::json!([{"@value": __val}]) }
                } else {
                    quote! { serde_json::json!([{"@value": self.#field_name}]) }
                }
            };

            if is_option {
                property_fields.push(quote! {
                    if let Some(ref __val) = self.#field_name {
                        obj.insert(#property_iri.to_string(), #value_expr);
                    }
                });
            } else {
                property_fields.push(quote! {
                    obj.insert(#property_iri.to_string(), #value_expr);
                });
            }
        }
    }

    // Validate: if type_field is set, we must have found a type_value field
    if container_attrs.type_field && type_insert.is_none() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "jsonld `type_field` requires exactly one field with `type_value` attribute",
        ));
    }

    let id_insert = id_field.unwrap_or_else(|| quote! {});
    let type_insert = type_insert.unwrap_or_else(|| quote! {});

    Ok(quote! {
        impl #impl_generics #trait_path::JsonLdExpandable for #name #ty_generics #where_clause {
            fn to_expanded(&self) -> serde_json::Value {
                let mut obj = serde_json::Map::new();

                #id_insert

                #type_insert

                #(#property_fields)*

                serde_json::Value::Object(obj)
            }
        }
    })
}

/// Generate `JsonLdExpandable` impl for an enum by delegating to inner types.
fn derive_enum(
    name: &syn::Ident,
    impl_generics: &syn::ImplGenerics<'_>,
    ty_generics: &syn::TypeGenerics<'_>,
    where_clause: Option<&syn::WhereClause>,
    trait_path: &syn::Path,
    container_attrs: &ContainerAttrs,
    variants: &syn::punctuated::Punctuated<syn::Variant, syn::token::Comma>,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    // Enums must not have type or type_field
    if container_attrs.type_iri.is_some() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "jsonld `type` is not allowed on enums; delegation uses the inner type's @type",
        ));
    }
    if container_attrs.type_field {
        return Err(syn::Error::new(proc_macro2::Span::call_site(), "jsonld `type_field` is not allowed on enums"));
    }

    let mut arms = Vec::new();

    for variant in variants {
        let variant_name = &variant.ident;

        match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                arms.push(quote! {
                    #name::#variant_name(inner) => #trait_path::JsonLdExpandable::to_expanded(inner),
                });
            }
            Fields::Unit => {
                return Err(syn::Error::new_spanned(
                    variant,
                    "JsonLdExpandable enum variants must be single-field tuple variants, not unit variants",
                ));
            }
            Fields::Named(_) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    "JsonLdExpandable enum variants must be single-field tuple variants, not named-field variants",
                ));
            }
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!("JsonLdExpandable enum variants must have exactly one field, found {}", fields.unnamed.len()),
                ));
            }
        }
    }

    Ok(quote! {
        impl #impl_generics #trait_path::JsonLdExpandable for #name #ty_generics #where_clause {
            fn to_expanded(&self) -> serde_json::Value {
                match self {
                    #(#arms)*
                }
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
    type_field: bool,
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
            } else if meta.path.is_ident("type_field") {
                result.type_field = true;
            } else if meta.path.is_ident("crate") {
                let value: syn::LitStr = meta.value()?.parse()?;
                result.crate_path = Some(value.value());
            } else {
                return Err(meta.error(format!(
                    "unknown jsonld container attribute `{}`; expected `type`, `type_field`, or `crate`",
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
    /// Emit `{"@value": val, "@type": datatype}` for typed values
    typed_value: bool,
    /// The datatype IRI for `typed_value`
    datatype: Option<String>,
    /// Emit `{"@value": val, "@type": "@json"}` for JSON values
    json_value: bool,
    /// Emit `{"@id": val}` for URI references
    id_ref: bool,
    /// Expand via `JsonLdLanguageMap::to_expanded_language_map()`
    language_map: bool,
    /// Expand map entries as separate IRI-keyed properties
    flatten_map: bool,
    /// Use field's `JsonLdExpandable::to_expanded()` directly
    custom: bool,
    /// This field provides the dynamic `@type` value
    type_value: bool,
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
            } else if meta.path.is_ident("typed_value") {
                result.typed_value = true;
            } else if meta.path.is_ident("datatype") {
                let value: syn::LitStr = meta.value()?.parse()?;
                result.datatype = Some(value.value());
            } else if meta.path.is_ident("json_value") {
                result.json_value = true;
            } else if meta.path.is_ident("id_ref") {
                result.id_ref = true;
            } else if meta.path.is_ident("language_map") {
                result.language_map = true;
            } else if meta.path.is_ident("flatten_map") {
                result.flatten_map = true;
            } else if meta.path.is_ident("custom") {
                result.custom = true;
            } else if meta.path.is_ident("type_value") {
                result.type_value = true;
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
                     id, skip, property, nested, vec, list, vocab, vocab_vec, \
                     typed_value, datatype, json_value, id_ref, language_map, \
                     flatten_map, custom, type_value",
                    meta.path.get_ident().map_or("?".into(), |i| i.to_string())
                )));
            }
            Ok(())
        })?;
    }

    // Validate mutually exclusive / conflicting attributes
    let span = attr_span.unwrap_or_else(proc_macro2::Span::call_site);

    // Value mode exclusivity: these are all mutually exclusive
    let value_modes: &[(&str, bool)] = &[
        ("nested", result.nested),
        ("vocab", result.vocab),
        ("vocab_vec", result.vocab_vec),
        ("typed_value", result.typed_value),
        ("json_value", result.json_value),
        ("language_map", result.language_map),
        ("flatten_map", result.flatten_map),
        ("custom", result.custom),
    ];
    let active: Vec<&str> = value_modes.iter().filter(|(_, set)| *set).map(|(name, _)| *name).collect();
    if active.len() > 1 {
        return Err(syn::Error::new(
            span,
            format!("conflicting jsonld attributes: {} are mutually exclusive", active.join(", ")),
        ));
    }

    // `list` is a modifier: valid alone or combined with `id_ref` only
    // When combined with other value modes (except id_ref), that's a conflict
    if result.list && !active.is_empty() {
        return Err(syn::Error::new(span, format!("jsonld attribute `list` conflicts with `{}`", active[0])));
    }

    // `id_ref` is a modifier: valid alone or combined with `list` only
    if result.id_ref && !active.is_empty() {
        return Err(syn::Error::new(span, format!("jsonld attribute `id_ref` conflicts with `{}`", active[0])));
    }

    if result.is_vec && !result.nested {
        return Err(syn::Error::new(span, "jsonld attribute `vec` can only be used together with `nested`"));
    }

    // typed_value requires datatype and vice versa
    if result.typed_value && result.datatype.is_none() {
        return Err(syn::Error::new(span, "jsonld attribute `typed_value` requires `datatype = \"IRI\"`"));
    }
    if result.datatype.is_some() && !result.typed_value {
        return Err(syn::Error::new(span, "jsonld attribute `datatype` requires `typed_value`"));
    }

    // flatten_map must NOT have property
    if result.flatten_map && result.property.is_some() {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `flatten_map` cannot be combined with `property` (keys come from the map)",
        ));
    }

    // custom requires property
    if result.custom && result.property.is_none() {
        return Err(syn::Error::new(span, "jsonld attribute `custom` requires `property = \"IRI\"`"));
    }

    // type_value cannot combine with anything else
    if result.type_value
        && (result.is_id
            || result.skip
            || result.property.is_some()
            || result.nested
            || result.list
            || result.vocab
            || result.vocab_vec
            || result.typed_value
            || result.json_value
            || result.id_ref
            || result.language_map
            || result.flatten_map
            || result.custom)
    {
        return Err(syn::Error::new(span, "jsonld attribute `type_value` cannot be combined with other attributes"));
    }

    if result.is_id
        && (result.nested
            || result.list
            || result.vocab
            || result.vocab_vec
            || result.property.is_some()
            || result.typed_value
            || result.json_value
            || result.id_ref
            || result.language_map
            || result.flatten_map
            || result.custom)
    {
        return Err(syn::Error::new(span, "jsonld attribute `id` cannot be combined with other property attributes"));
    }

    if result.skip
        && (result.is_id
            || result.nested
            || result.list
            || result.vocab
            || result.vocab_vec
            || result.property.is_some()
            || result.typed_value
            || result.json_value
            || result.id_ref
            || result.language_map
            || result.flatten_map
            || result.custom
            || result.type_value)
    {
        return Err(syn::Error::new(span, "jsonld attribute `skip` cannot be combined with other attributes"));
    }

    Ok(result)
}
