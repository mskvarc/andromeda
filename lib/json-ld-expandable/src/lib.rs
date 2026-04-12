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
/// - `type = "IRI"` - The JSON-LD @type IRI for this struct (required for structs, unless `type_field` or `fragment` is used)
/// - `type_field` - Indicates @type is provided by a field marked with `type_value` (mutually exclusive with `type` and `fragment`)
/// - `fragment` - Expand this struct as a JSON-LD object fragment without emitting `@type`
/// - `crate = "path"` - Custom path to the crate containing JsonLdExpandable trait
///   (defaults to `crate::jsonld`)
/// - `debug` - Emit the generated code as a compile error (for debugging macro output)
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
/// - `vocab_polymorphic` - Polymorphic vocab (handles string/array/null at runtime via match)
/// - `typed_value` - Emit `{"@value": val, "@type": datatype}` (requires `datatype`)
/// - `datatype = "IRI"` - The @type IRI for typed values (requires `typed_value`)
/// - `json_value` - Emit `{"@value": val, "@type": "@json"}`
/// - `id_ref` - Emit `{"@id": val}` for URI references (can combine with `list`)
/// - `language_map` - Expand via `JsonLdLanguageMap::to_expanded_language_map()`
/// - `flatten_map` - Expand map entries as separate IRI-keyed properties
/// - `flatten_object` - Expand a nested JsonLdExpandable object and merge its properties into the parent
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

    let tokens = match &input.data {
        Data::Struct(data) => derive_struct(name, &impl_generics, &ty_generics, where_clause, &trait_path, &container_attrs, &data.fields)?,
        Data::Enum(data) => derive_enum(name, &impl_generics, &ty_generics, where_clause, &trait_path, &container_attrs, &data.variants)?,
        Data::Union(_) => return Err(syn::Error::new_spanned(&input.ident, "JsonLdExpandable does not support unions")),
    };

    // Debug mode: emit the generated code as a compile error
    if container_attrs.debug {
        let code = tokens.to_string();
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("[jsonld debug] Generated code for {}:\n\n{}", name, code),
        ));
    }

    Ok(tokens)
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
            "jsonld attributes `type` and `type_field` are mutually exclusive\n\n\
             help: use `type` for a static @type, or `type_field` with a `type_value` field for dynamic @type",
        ));
    }
    if container_attrs.fragment && (container_attrs.type_iri.is_some() || container_attrs.type_field) {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "jsonld attribute `fragment` is mutually exclusive with `type` and `type_field`",
        ));
    }
    if container_attrs.type_iri.is_none() && !container_attrs.type_field && !container_attrs.fragment {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "JsonLdExpandable requires either `type`, `type_field`, or `fragment`\n\n\
             help: add a static @type:\n  \
             #[jsonld(type = \"https://example.com/MyType\")]\n\n\
             or use dynamic @type from a field:\n  \
             #[jsonld(type_field)]\n  \
             pub struct MyType {\n      \
             #[jsonld(type_value)]\n      \
             pub type_: MyTypeValue,\n      \
             ...\n  \
             }\n\n\
             or mark this struct as a reusable fragment:\n  \
             #[jsonld(fragment)]",
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
                    "jsonld attribute `type_value` requires `type_field` on the container\n\n\
                     help: add `type_field` to the struct:\n  \
                     #[jsonld(type_field)]",
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

        if attrs.flatten_object {
            let is_option = is_option_type(&field.ty);
            if is_option {
                property_fields.push(quote! {
                    if let Some(ref __val) = self.#field_name {
                        if let serde_json::Value::Object(__fragment_obj) = #trait_path::JsonLdExpandable::to_expanded(__val) {
                            for (__key, __value) in __fragment_obj {
                                match __key.as_str() {
                                    "@id" | "@type" => {}
                                    _other => {
                                        obj.insert(__key, __value);
                                    }
                                }
                            }
                        }
                    }
                });
            } else {
                property_fields.push(quote! {
                    if let serde_json::Value::Object(__fragment_obj) = #trait_path::JsonLdExpandable::to_expanded(&self.#field_name) {
                        for (__key, __value) in __fragment_obj {
                            match __key.as_str() {
                                "@id" | "@type" => {}
                                _other => {
                                    obj.insert(__key, __value);
                                }
                            }
                        }
                    }
                });
            }
            continue;
        }

        if let Some(property_iri) = &attrs.property {
            let is_option = is_option_type(&field.ty);

            let value_expr = if attrs.typed_value {
                let datatype = attrs.datatype.as_ref().unwrap();
                if attrs.list {
                    // list + typed_value: wrap typed values in @list
                    if is_option {
                        quote! {
                            serde_json::json!([{"@list": __val.iter()
                                .map(|__item| serde_json::json!({"@value": __item, "@type": #datatype}))
                                .collect::<Vec<_>>()}])
                        }
                    } else {
                        quote! {
                            serde_json::json!([{"@list": self.#field_name.iter()
                                .map(|__item| serde_json::json!({"@value": __item, "@type": #datatype}))
                                .collect::<Vec<_>>()}])
                        }
                    }
                } else if is_option {
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
            } else if attrs.vocab_polymorphic {
                // vocab_polymorphic generates a match expression that handles string/array/null
                // This bypasses the normal value_expr pattern since null must skip entirely
                property_fields.push(quote! {
                    match &self.#field_name {
                        serde_json::Value::String(__s) => {
                            obj.insert(#property_iri.to_string(), serde_json::json!([{"@id": __s}]));
                        }
                        serde_json::Value::Array(__arr) => {
                            let __ids: Vec<serde_json::Value> = __arr
                                .iter()
                                .filter_map(|__v| __v.as_str().map(|__s| serde_json::json!({"@id": __s})))
                                .collect();
                            obj.insert(#property_iri.to_string(), serde_json::Value::Array(__ids));
                        }
                        serde_json::Value::Null => {}
                        __other => {
                            obj.insert(#property_iri.to_string(), serde_json::json!([{"@value": __other}]));
                        }
                    }
                });
                continue;
            } else if attrs.nested {
                if attrs.list {
                    // list + nested: wrap expanded items in @list
                    if is_option {
                        quote! {
                            serde_json::json!([{"@list": __val.iter()
                                .map(|__item| #trait_path::JsonLdExpandable::to_expanded(__item))
                                .collect::<Vec<_>>()}])
                        }
                    } else {
                        quote! {
                            serde_json::json!([{"@list": self.#field_name.iter()
                                .map(|__item| #trait_path::JsonLdExpandable::to_expanded(__item))
                                .collect::<Vec<_>>()}])
                        }
                    }
                } else if attrs.is_vec {
                    quote! {
                        serde_json::json!(self.#field_name.iter()
                            .map(|item| #trait_path::JsonLdExpandable::to_expanded(item))
                            .collect::<Vec<_>>())
                    }
                } else if is_option {
                    quote! {
                        serde_json::json!([#trait_path::JsonLdExpandable::to_expanded(__val)])
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

/// Generate `JsonLdExpandable` impl for an enum.
///
/// Supports three variant forms:
/// - Single-field tuple variants: delegate to inner type's `to_expanded()`
/// - Unit variants with `#[jsonld(type = "...")]`: emit `{"@type": [...]}`
/// - Named-field variants with `#[jsonld(type = "...")]`: expand like structs
fn derive_enum(
    name: &syn::Ident,
    impl_generics: &syn::ImplGenerics<'_>,
    ty_generics: &syn::TypeGenerics<'_>,
    where_clause: Option<&syn::WhereClause>,
    trait_path: &syn::Path,
    container_attrs: &ContainerAttrs,
    variants: &syn::punctuated::Punctuated<syn::Variant, syn::token::Comma>,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    // Enums must not have type or type_field at container level
    if container_attrs.type_iri.is_some() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "jsonld `type` is not allowed on enums; use variant-level `type` attributes instead",
        ));
    }
    if container_attrs.type_field {
        return Err(syn::Error::new(proc_macro2::Span::call_site(), "jsonld `type_field` is not allowed on enums"));
    }

    let mut arms = Vec::new();

    for variant in variants {
        let variant_name = &variant.ident;
        let variant_attrs = extract_variant_attrs(&variant.attrs)?;

        match &variant.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                // Single-field tuple variant: delegate to inner type
                if variant_attrs.type_iri.is_some() {
                    return Err(syn::Error::new_spanned(
                        variant,
                        "jsonld `type` on single-field tuple variants is not supported; \
                         the inner type provides its own @type",
                    ));
                }
                arms.push(quote! {
                    #name::#variant_name(inner) => #trait_path::JsonLdExpandable::to_expanded(inner),
                });
            }
            Fields::Unit => {
                // Unit variant: requires type attribute
                let type_iri = variant_attrs
                    .type_iri
                    .ok_or_else(|| syn::Error::new_spanned(variant, "unit variants require #[jsonld(type = \"...\")] to specify the @type IRI"))?;
                arms.push(quote! {
                    #name::#variant_name => {
                        let mut obj = serde_json::Map::new();
                        obj.insert("@type".to_string(), serde_json::json!([#type_iri]));
                        serde_json::Value::Object(obj)
                    }
                });
            }
            Fields::Named(fields) => {
                // Named-field variant: requires type attribute, expands like a struct
                let type_iri = variant_attrs
                    .type_iri
                    .ok_or_else(|| syn::Error::new_spanned(variant, "named-field variants require #[jsonld(type = \"...\")] to specify the @type IRI"))?;

                let mut id_field: Option<proc_macro2::TokenStream> = None;
                let mut property_fields: Vec<proc_macro2::TokenStream> = Vec::new();

                for field in &fields.named {
                    let field_name = field.ident.as_ref().unwrap();
                    let attrs = parse_field_attrs(&field.attrs)?;

                    if attrs.skip {
                        continue;
                    }

                    if attrs.type_value {
                        return Err(syn::Error::new(field.span(), "jsonld attribute `type_value` is not supported in enum variants"));
                    }

                    if attrs.is_id {
                        id_field = Some(quote! {
                            obj.insert("@id".to_string(), serde_json::json!(#field_name));
                        });
                        continue;
                    }

                    if attrs.flatten_map {
                        property_fields.push(quote! {
                            for (__key, __val) in #field_name {
                                obj.insert(__key.to_string(), #trait_path::JsonLdExpandable::to_expanded(__val));
                            }
                        });
                        continue;
                    }

                    if attrs.flatten_object {
                        let is_option = is_option_type(&field.ty);
                        if is_option {
                            property_fields.push(quote! {
                                if let Some(__val) = #field_name.as_ref() {
                                    if let serde_json::Value::Object(__fragment_obj) = #trait_path::JsonLdExpandable::to_expanded(__val) {
                                        for (__key, __value) in __fragment_obj {
                                            match __key.as_str() {
                                                "@id" | "@type" => {}
                                                _other => {
                                                    obj.insert(__key, __value);
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                        } else {
                            property_fields.push(quote! {
                                if let serde_json::Value::Object(__fragment_obj) = #trait_path::JsonLdExpandable::to_expanded(#field_name) {
                                    for (__key, __value) in __fragment_obj {
                                        match __key.as_str() {
                                            "@id" | "@type" => {}
                                            _other => {
                                                obj.insert(__key, __value);
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        continue;
                    }

                    if let Some(property_iri) = &attrs.property {
                        let is_option = is_option_type(&field.ty);
                        let value_expr = generate_field_value_expr(&attrs, field_name, is_option, trait_path);

                        if is_option {
                            // Use .as_ref() to convert &Option<T> to Option<&T>
                            property_fields.push(quote! {
                                if let Some(__val) = #field_name.as_ref() {
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

                let id_insert = id_field.unwrap_or_else(|| quote! {});
                let field_names: Vec<_> = fields.named.iter().map(|f| &f.ident).collect();

                arms.push(quote! {
                    #name::#variant_name { #(#field_names),* } => {
                        let mut obj = serde_json::Map::new();
                        #id_insert
                        obj.insert("@type".to_string(), serde_json::json!([#type_iri]));
                        #(#property_fields)*
                        serde_json::Value::Object(obj)
                    }
                });
            }
            Fields::Unnamed(fields) => {
                return Err(syn::Error::new_spanned(
                    variant,
                    format!("tuple variants must have exactly one field for delegation, found {}", fields.unnamed.len()),
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

/// Extract variant-level jsonld attributes.
fn extract_variant_attrs(attrs: &[syn::Attribute]) -> Result<VariantAttrs, syn::Error> {
    let mut result = VariantAttrs::default();

    for attr in attrs {
        if !attr.path().is_ident("jsonld") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("type") {
                let value: syn::LitStr = meta.value()?.parse()?;
                let iri = resolve_iri_prefix(&value.value());
                if !looks_like_iri(&iri) {
                    return Err(syn::Error::new(
                        value.span(),
                        format!(
                            "invalid IRI \"{}\": expected a full IRI with a scheme \
                             (e.g. \"https://example.com/Type\", \"ngsi:Property\", or \"urn:example:Type\")",
                            value.value()
                        ),
                    ));
                }
                result.type_iri = Some(iri);
            } else {
                return Err(meta.error(format!(
                    "unknown jsonld variant attribute `{}`; expected `type`",
                    meta.path.get_ident().map_or("?".into(), |i| i.to_string())
                )));
            }
            Ok(())
        })?;
    }

    Ok(result)
}

#[derive(Default)]
struct VariantAttrs {
    type_iri: Option<String>,
}

/// Generate value expression for a field (shared between structs and enum variants).
fn generate_field_value_expr(attrs: &FieldAttrs, field_name: &syn::Ident, is_option: bool, trait_path: &syn::Path) -> proc_macro2::TokenStream {
    if attrs.typed_value {
        let datatype = attrs.datatype.as_ref().unwrap();
        if attrs.list {
            if is_option {
                quote! {
                    serde_json::json!([{"@list": __val.iter()
                        .map(|__item| serde_json::json!({"@value": __item, "@type": #datatype}))
                        .collect::<Vec<_>>()}])
                }
            } else {
                quote! {
                    serde_json::json!([{"@list": #field_name.iter()
                        .map(|__item| serde_json::json!({"@value": __item, "@type": #datatype}))
                        .collect::<Vec<_>>()}])
                }
            }
        } else if is_option {
            quote! { serde_json::json!([{"@value": __val, "@type": #datatype}]) }
        } else {
            quote! { serde_json::json!([{"@value": #field_name, "@type": #datatype}]) }
        }
    } else if attrs.json_value {
        if is_option {
            quote! { serde_json::json!([{"@value": __val, "@type": "@json"}]) }
        } else {
            quote! { serde_json::json!([{"@value": #field_name, "@type": "@json"}]) }
        }
    } else if attrs.id_ref {
        if attrs.list {
            if is_option {
                quote! {
                    serde_json::json!([{"@list": __val.iter().map(|__item| serde_json::json!({"@id": __item.to_string()})).collect::<Vec<_>>()}])
                }
            } else {
                quote! {
                    serde_json::json!([{"@list": #field_name.iter().map(|__item| serde_json::json!({"@id": __item.to_string()})).collect::<Vec<_>>()}])
                }
            }
        } else if is_option {
            quote! { serde_json::json!([{"@id": __val.to_string()}]) }
        } else {
            quote! { serde_json::json!([{"@id": #field_name.to_string()}]) }
        }
    } else if attrs.language_map {
        if is_option {
            quote! { #trait_path::JsonLdLanguageMap::to_expanded_language_map(__val) }
        } else {
            quote! { #trait_path::JsonLdLanguageMap::to_expanded_language_map(&#field_name) }
        }
    } else if attrs.custom {
        if is_option {
            quote! { #trait_path::JsonLdExpandable::to_expanded(__val) }
        } else {
            quote! { #trait_path::JsonLdExpandable::to_expanded(&#field_name) }
        }
    } else if attrs.vocab {
        if is_option {
            quote! { serde_json::json!([{"@id": __val}]) }
        } else {
            quote! { serde_json::json!([{"@id": #field_name}]) }
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
                serde_json::json!(#field_name.iter()
                    .map(|item| serde_json::json!({"@id": item}))
                    .collect::<Vec<_>>())
            }
        }
    } else if attrs.nested {
        if attrs.list {
            if is_option {
                quote! {
                    serde_json::json!([{"@list": __val.iter()
                        .map(|__item| #trait_path::JsonLdExpandable::to_expanded(__item))
                        .collect::<Vec<_>>()}])
                }
            } else {
                quote! {
                    serde_json::json!([{"@list": #field_name.iter()
                        .map(|__item| #trait_path::JsonLdExpandable::to_expanded(__item))
                        .collect::<Vec<_>>()}])
                }
            }
        } else if attrs.is_vec {
            quote! {
                serde_json::json!(#field_name.iter()
                    .map(|item| #trait_path::JsonLdExpandable::to_expanded(item))
                    .collect::<Vec<_>>())
            }
        } else if is_option {
            quote! {
                serde_json::json!([#trait_path::JsonLdExpandable::to_expanded(__val)])
            }
        } else {
            quote! {
                serde_json::json!([#trait_path::JsonLdExpandable::to_expanded(&#field_name)])
            }
        }
    } else if attrs.list {
        if is_option {
            quote! { serde_json::json!([{"@list": __val}]) }
        } else {
            quote! { serde_json::json!([{"@list": #field_name}]) }
        }
    } else {
        // default: plain @value
        if is_option {
            quote! { serde_json::json!([{"@value": __val}]) }
        } else {
            quote! { serde_json::json!([{"@value": #field_name}]) }
        }
    }
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

/// Resolve IRI prefix aliases to full IRIs.
///
/// Supported prefixes:
/// - `ngsi:` -> `https://uri.etsi.org/ngsi-ld/`
/// - `xsd:` -> `http://www.w3.org/2001/XMLSchema#`
///
/// If the input is already a full IRI or uses an unknown prefix, returns it unchanged.
fn resolve_iri_prefix(s: &str) -> String {
    if let Some(local) = s.strip_prefix("ngsi:") {
        format!("https://uri.etsi.org/ngsi-ld/{}", local)
    } else if let Some(local) = s.strip_prefix("xsd:") {
        format!("http://www.w3.org/2001/XMLSchema#{}", local)
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Container attributes
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ContainerAttrs {
    type_iri: Option<String>,
    type_field: bool,
    fragment: bool,
    crate_path: Option<String>,
    /// Emit the generated code as a compile error for debugging
    debug: bool,
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
                let iri = resolve_iri_prefix(&value.value());
                if !looks_like_iri(&iri) {
                    return Err(syn::Error::new(
                        value.span(),
                        format!(
                            "invalid IRI \"{}\": expected a full IRI with a scheme \
                             (e.g. \"https://example.com/Type\", \"ngsi:Property\", or \"urn:example:Type\")",
                            value.value()
                        ),
                    ));
                }
                result.type_iri = Some(iri);
            } else if meta.path.is_ident("type_field") {
                result.type_field = true;
            } else if meta.path.is_ident("fragment") {
                result.fragment = true;
            } else if meta.path.is_ident("crate") {
                let value: syn::LitStr = meta.value()?.parse()?;
                result.crate_path = Some(value.value());
            } else if meta.path.is_ident("debug") {
                result.debug = true;
            } else {
                return Err(meta.error(format!(
                    "unknown jsonld container attribute `{}`; expected `type`, `type_field`, `fragment`, `crate`, or `debug`",
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
    /// For polymorphic vocab values (string/array/null) - handles runtime dispatch
    vocab_polymorphic: bool,
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
    /// Expand a nested JsonLdExpandable object and merge its properties into the parent
    flatten_object: bool,
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
            } else if meta.path.is_ident("vocab_polymorphic") {
                result.vocab_polymorphic = true;
            } else if meta.path.is_ident("typed_value") {
                result.typed_value = true;
            } else if meta.path.is_ident("datatype") {
                let value: syn::LitStr = meta.value()?.parse()?;
                result.datatype = Some(resolve_iri_prefix(&value.value()));
            } else if meta.path.is_ident("json_value") {
                result.json_value = true;
            } else if meta.path.is_ident("id_ref") {
                result.id_ref = true;
            } else if meta.path.is_ident("language_map") {
                result.language_map = true;
            } else if meta.path.is_ident("flatten_map") {
                result.flatten_map = true;
            } else if meta.path.is_ident("flatten_object") {
                result.flatten_object = true;
            } else if meta.path.is_ident("custom") {
                result.custom = true;
            } else if meta.path.is_ident("type_value") {
                result.type_value = true;
            } else if meta.path.is_ident("property") {
                let value: syn::LitStr = meta.value()?.parse()?;
                let iri = resolve_iri_prefix(&value.value());
                if !looks_like_iri(&iri) {
                    return Err(syn::Error::new(
                        value.span(),
                        format!(
                            "invalid property IRI \"{}\": expected a full IRI with a scheme \
                             (e.g. \"https://example.com/prop\", \"ngsi:hasValue\", or \"urn:example:prop\")",
                            value.value()
                        ),
                    ));
                }
                result.property = Some(iri);
            } else {
                return Err(meta.error(format!(
                    "unknown jsonld field attribute `{}`; expected one of: \
                     id, skip, property, nested, vec, list, vocab, vocab_vec, \
                     vocab_polymorphic, typed_value, datatype, json_value, id_ref, \
                     language_map, flatten_map, flatten_object, custom, type_value",
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
        ("vocab_polymorphic", result.vocab_polymorphic),
        ("typed_value", result.typed_value),
        ("json_value", result.json_value),
        ("language_map", result.language_map),
        ("flatten_map", result.flatten_map),
        ("flatten_object", result.flatten_object),
        ("custom", result.custom),
    ];
    let active: Vec<&str> = value_modes.iter().filter(|(_, set)| *set).map(|(name, _)| *name).collect();
    if active.len() > 1 {
        return Err(syn::Error::new(
            span,
            format!("conflicting jsonld attributes: {} are mutually exclusive", active.join(", ")),
        ));
    }

    // `list` is a modifier: valid alone or combined with `id_ref`, `nested`, or `typed_value`
    // When combined with other value modes, that's a conflict
    if result.list {
        let list_compatible = ["nested", "typed_value"];
        for mode in &active {
            if !list_compatible.contains(mode) {
                return Err(syn::Error::new(span, format!("jsonld attribute `list` conflicts with `{}`", mode)));
            }
        }
    }

    // `id_ref` is a modifier: valid alone or combined with `list` only
    if result.id_ref && !active.is_empty() {
        return Err(syn::Error::new(span, format!("jsonld attribute `id_ref` conflicts with `{}`", active[0])));
    }

    if result.is_vec && !result.nested {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `vec` can only be used together with `nested`\n\n\
             help: add `nested` for a Vec of expandable objects:\n  \
             #[jsonld(property = \"...\", nested, vec)]",
        ));
    }

    // typed_value requires datatype and vice versa
    if result.typed_value && result.datatype.is_none() {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `typed_value` requires `datatype`\n\n\
             help: add a datatype IRI for the typed value:\n  \
             #[jsonld(property = \"...\", typed_value, datatype = \"https://uri.etsi.org/ngsi-ld/DateTime\")]",
        ));
    }
    if result.datatype.is_some() && !result.typed_value {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `datatype` requires `typed_value`\n\n\
             help: add `typed_value` to emit {\"@value\": ..., \"@type\": datatype}:\n  \
             #[jsonld(property = \"...\", typed_value, datatype = \"...\")]",
        ));
    }

    // flatten_map must NOT have property
    if result.flatten_map && result.property.is_some() {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `flatten_map` cannot be combined with `property`\n\n\
             help: `flatten_map` uses map keys as property IRIs, remove `property`:\n  \
             #[jsonld(flatten_map)]",
        ));
    }

    if result.flatten_object && result.property.is_some() {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `flatten_object` cannot be combined with `property`\n\n\
             help: `flatten_object` merges expanded object properties into the parent:\n  \
             #[jsonld(flatten_object)]",
        ));
    }

    if result.flatten_object && result.flatten_map {
        return Err(syn::Error::new(
            span,
            "jsonld attributes `flatten_object` and `flatten_map` are mutually exclusive",
        ));
    }

    // custom requires property
    if result.custom && result.property.is_none() {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `custom` requires `property`\n\n\
             help: add the property IRI:\n  \
             #[jsonld(property = \"https://example.com/prop\", custom)]",
        ));
    }

    // vocab_polymorphic requires property
    if result.vocab_polymorphic && result.property.is_none() {
        return Err(syn::Error::new(
            span,
            "jsonld attribute `vocab_polymorphic` requires `property`\n\n\
             help: add the property IRI:\n  \
             #[jsonld(property = \"https://uri.etsi.org/ngsi-ld/hasVocab\", vocab_polymorphic)]",
        ));
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
            || result.vocab_polymorphic
            || result.typed_value
            || result.json_value
            || result.id_ref
            || result.language_map
            || result.flatten_map
            || result.flatten_object
            || result.custom)
    {
        return Err(syn::Error::new(span, "jsonld attribute `type_value` cannot be combined with other attributes"));
    }

    if result.is_id
        && (result.nested
            || result.list
            || result.vocab
            || result.vocab_vec
            || result.vocab_polymorphic
            || result.property.is_some()
            || result.typed_value
            || result.json_value
            || result.id_ref
            || result.language_map
            || result.flatten_map
            || result.flatten_object
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
            || result.vocab_polymorphic
            || result.property.is_some()
            || result.typed_value
            || result.json_value
            || result.id_ref
            || result.language_map
            || result.flatten_map
            || result.flatten_object
            || result.custom
            || result.type_value)
    {
        return Err(syn::Error::new(span, "jsonld attribute `skip` cannot be combined with other attributes"));
    }

    Ok(result)
}
