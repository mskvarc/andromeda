//! Proc-macro crate that generates NGSI-LD vocabulary constants from a core context JSON file.
//!
//! Parses the NGSI-LD core context JSON at **compile time** and emits `pub const &'static str`
//! constants for every resolved term, namespace prefix constants, lookup tables, and binary-search
//! functions.  No runtime parsing, no `LazyLock`, no `expect()`.
//!
//! # Usage
//!
//! ```ignore
//! ngsi_ld_vocabulary::generate!("src/jsonld/context/ngsi-ld-core-context-v1.9.jsonld");
//! ```

use heck::ToShoutySnakeCase;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use std::collections::HashMap;
use syn::{Error, LitStr, parse_macro_input};

/// A resolved context term: compact name, expanded IRI, and derived constant name.
struct ResolvedTerm {
    /// The compact name as it appears in the context (e.g. `"createdAt"`).
    compact: String,
    /// The fully expanded IRI (e.g. `"https://uri.etsi.org/ngsi-ld/createdAt"`).
    expanded: String,
    /// The SCREAMING_SNAKE_CASE constant name derived from the local part of the expanded IRI.
    const_name: String,
}

/// A namespace prefix extracted from the context (e.g. `"ngsi-ld"` -> `"https://uri.etsi.org/ngsi-ld/"`).
struct Prefix {
    /// The short prefix name (e.g. `"ngsi-ld"`).
    short: String,
    /// The expanded namespace URI (e.g. `"https://uri.etsi.org/ngsi-ld/"`).
    expanded: String,
    /// The SCREAMING_SNAKE_CASE constant name (e.g. `"NGSI_LD"`).
    const_name: String,
}

/// Generate NGSI-LD vocabulary constants from a core context JSON file.
///
/// Accepts a single string literal argument: the path to the context JSON file,
/// relative to `CARGO_MANIFEST_DIR`.
///
/// # Generated items
///
/// - `pub mod prefix { ... }` with namespace prefix constants
/// - `pub const PROPERTY: &str = "...";` (one per resolved term)
/// - `pub const TERM_COUNT: usize = N;`
/// - `pub const COMPACT_TO_EXPANDED: &[(&str, &str)]` sorted by compact name
/// - `pub const EXPANDED_TO_COMPACT: &[(&str, &str)]` sorted by expanded IRI
/// - `pub fn compact_to_expanded(compact: &str) -> Option<&'static str>`
/// - `pub fn expanded_to_compact(expanded: &str) -> Option<&'static str>`
#[proc_macro]
pub fn generate(input: TokenStream) -> TokenStream {
    let lit = parse_macro_input!(input as LitStr);
    let rel_path = lit.value();

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => dir,
        Err(e) => {
            return Error::new(lit.span(), format!("CARGO_MANIFEST_DIR not set: {e}")).to_compile_error().into();
        }
    };

    let file_path = std::path::Path::new(&manifest_dir).join(&rel_path);
    let json_str = match std::fs::read_to_string(&file_path) {
        Ok(s) => s,
        Err(e) => {
            return Error::new(lit.span(), format!("cannot read {}: {e}", file_path.display()))
                .to_compile_error()
                .into();
        }
    };

    let root: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            return Error::new(lit.span(), format!("invalid JSON in {}: {e}", file_path.display()))
                .to_compile_error()
                .into();
        }
    };

    let context_obj = match root.get("@context").and_then(|v| v.as_object()) {
        Some(obj) => obj,
        None => {
            return Error::new(lit.span(), "JSON missing top-level @context object").to_compile_error().into();
        }
    };

    // Step 1: collect prefixes (entries whose value is a string ending in `/` or `#`).
    let prefixes: Vec<Prefix> = context_obj
        .iter()
        .filter(|(k, _)| !k.starts_with('@'))
        .filter_map(|(k, v)| {
            v.as_str().and_then(|s| {
                if s.ends_with('/') || s.ends_with('#') {
                    Some(Prefix {
                        short: k.clone(),
                        expanded: s.to_string(),
                        const_name: k.to_shouty_snake_case(),
                    })
                } else {
                    None
                }
            })
        })
        .collect();

    // Step 2: resolve all non-keyword, non-prefix, non-alias entries.
    let mut terms: Vec<ResolvedTerm> = Vec::new();
    let mut const_name_map: HashMap<String, String> = HashMap::new();

    for (key, _) in context_obj {
        if key.starts_with('@') {
            continue;
        }

        let expanded = match resolve_term(context_obj, &prefixes, key) {
            Some(iri) => iri,
            None => continue,
        };

        // Skip JSON-LD keyword aliases
        if expanded.starts_with('@') {
            continue;
        }

        // Derive constant name from the local part of the expanded IRI.
        // On collision, the first term keeps the base name and the second gets a
        // `_TYPE` or `_PROP` suffix depending on whether the compact name starts
        // with an uppercase letter (type-like) or lowercase (property-like).
        let local_name = extract_local_name(&expanded);
        let mut const_name = local_name.to_shouty_snake_case();

        if const_name_map.contains_key(&const_name) {
            // Suffix based on compact name casing to disambiguate
            let suffix = if key.starts_with(|c: char| c.is_uppercase()) { "_TYPE" } else { "_PROP" };
            const_name = format!("{const_name}{suffix}");
        }

        // Ensure no collision remains after disambiguation
        if let Some(prev_compact) = const_name_map.get(&const_name) {
            return Error::new(
                lit.span(),
                format!("constant name collision: both '{}' and '{}' produce '{}'", prev_compact, key, const_name),
            )
            .to_compile_error()
            .into();
        }
        const_name_map.insert(const_name.clone(), key.clone());

        terms.push(ResolvedTerm {
            compact: key.clone(),
            expanded,
            const_name,
        });
    }

    // Step 3: generate code.
    let rel_path_str = rel_path.as_str();

    // Rebuild tracking: include_str! so cargo knows to rebuild when the file changes.
    let rebuild_tracking = quote! {
        const _: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/", #rel_path_str));
    };

    // Prefix module
    let prefix_consts: Vec<proc_macro2::TokenStream> = prefixes
        .iter()
        .map(|p| {
            let name = syn::Ident::new(&p.const_name, Span::call_site());
            let expanded = &p.expanded;
            let doc = format!("`{}` -> `{}`", p.short, p.expanded);
            quote! {
                #[doc = #doc]
                pub const #name: &str = #expanded;
            }
        })
        .collect();

    let prefix_mod = quote! {
        /// Namespace prefix constants.
        pub mod prefix {
            #(#prefix_consts)*
        }
    };

    // Term constants
    let term_consts: Vec<proc_macro2::TokenStream> = terms
        .iter()
        .map(|t| {
            let name = syn::Ident::new(&t.const_name, Span::call_site());
            let expanded = &t.expanded;
            let doc = format!("`{}` -> `{}`", t.compact, t.expanded);
            quote! {
                #[doc = #doc]
                pub const #name: &str = #expanded;
            }
        })
        .collect();

    // TERM_COUNT
    let term_count = terms.len();

    // COMPACT_TO_EXPANDED: sorted by compact name
    let mut compact_sorted: Vec<(&str, &str)> = terms.iter().map(|t| (t.compact.as_str(), t.expanded.as_str())).collect();
    compact_sorted.sort_by_key(|(compact, _)| *compact);

    let compact_to_expanded_entries: Vec<proc_macro2::TokenStream> =
        compact_sorted.iter().map(|(compact, expanded)| quote! { (#compact, #expanded) }).collect();

    // EXPANDED_TO_COMPACT: sorted by expanded IRI
    let mut expanded_sorted: Vec<(&str, &str)> = terms.iter().map(|t| (t.expanded.as_str(), t.compact.as_str())).collect();
    expanded_sorted.sort_by_key(|(expanded, _)| *expanded);

    let expanded_to_compact_entries: Vec<proc_macro2::TokenStream> =
        expanded_sorted.iter().map(|(expanded, compact)| quote! { (#expanded, #compact) }).collect();

    let output = quote! {
        #rebuild_tracking

        #prefix_mod

        #(#term_consts)*

        /// Total number of context-derived terms.
        pub const TERM_COUNT: usize = #term_count;

        /// (compact_name, expanded_iri) pairs, sorted by compact name.
        pub const COMPACT_TO_EXPANDED: &[(&str, &str)] = &[
            #(#compact_to_expanded_entries),*
        ];

        /// (expanded_iri, compact_name) pairs, sorted by expanded IRI.
        pub const EXPANDED_TO_COMPACT: &[(&str, &str)] = &[
            #(#expanded_to_compact_entries),*
        ];

        /// Look up expanded IRI by compact name via binary search.
        pub fn compact_to_expanded(compact: &str) -> Option<&'static str> {
            COMPACT_TO_EXPANDED
                .binary_search_by_key(&compact, |(c, _)| c)
                .ok()
                .map(|i| COMPACT_TO_EXPANDED[i].1)
        }

        /// Look up compact name by expanded IRI via binary search.
        pub fn expanded_to_compact(expanded: &str) -> Option<&'static str> {
            EXPANDED_TO_COMPACT
                .binary_search_by_key(&expanded, |(e, _)| e)
                .ok()
                .map(|i| EXPANDED_TO_COMPACT[i].1)
        }
    };

    output.into()
}

/// Extract the local name from an IRI (the part after the last `/` or `#`).
fn extract_local_name(iri: &str) -> &str {
    let after_hash = iri.rsplit_once('#').map(|(_, local)| local);
    let after_slash = iri.rsplit_once('/').map(|(_, local)| local);

    // Prefer hash fragment if present (it's more specific)
    match (after_hash, after_slash) {
        (Some(h), Some(s)) => {
            // Take whichever is shorter (= further right in the IRI)
            if h.len() <= s.len() { h } else { s }
        }
        (Some(h), None) => h,
        (None, Some(s)) => s,
        (None, None) => iri,
    }
}

/// Resolve a compact term from the context to its expanded IRI string.
fn resolve_term(context: &serde_json::Map<String, serde_json::Value>, prefixes: &[Prefix], compact_term: &str) -> Option<String> {
    let value = context.get(compact_term)?;

    match value {
        serde_json::Value::String(s) => {
            if s.starts_with('@') {
                return None;
            }
            if s.ends_with('/') || s.ends_with('#') {
                return None;
            }
            expand_prefixed(s, prefixes)
        }
        serde_json::Value::Object(obj) => {
            let id = obj.get("@id").and_then(|v| v.as_str())?;
            if id.starts_with('@') {
                return None;
            }
            expand_prefixed(id, prefixes)
        }
        _ => None,
    }
}

/// Expand a possibly-prefixed term to a full IRI.
fn expand_prefixed(term: &str, prefixes: &[Prefix]) -> Option<String> {
    if term.contains("://") {
        return Some(term.to_string());
    }

    if let Some(colon_pos) = term.find(':') {
        let prefix = &term[..colon_pos];
        let local = &term[colon_pos + 1..];
        for p in prefixes {
            if p.short == prefix {
                return Some(format!("{}{}", p.expanded, local));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_local_name_from_slash_iri() {
        assert_eq!(extract_local_name("https://uri.etsi.org/ngsi-ld/Property"), "Property");
    }

    #[test]
    fn extract_local_name_from_hash_iri() {
        assert_eq!(extract_local_name("https://purl.org/geojson/vocab#coordinates"), "coordinates");
    }

    #[test]
    fn extract_local_name_from_mixed_iri() {
        assert_eq!(extract_local_name("https://example.com/path#fragment"), "fragment");
    }

    #[test]
    fn expand_prefixed_known_prefix() {
        let prefixes = vec![Prefix {
            short: "ngsi-ld".to_string(),
            expanded: "https://uri.etsi.org/ngsi-ld/".to_string(),
            const_name: "NGSI_LD".to_string(),
        }];
        assert_eq!(
            expand_prefixed("ngsi-ld:Property", &prefixes),
            Some("https://uri.etsi.org/ngsi-ld/Property".to_string())
        );
    }

    #[test]
    fn expand_prefixed_absolute_iri_unchanged() {
        let prefixes = vec![];
        assert_eq!(
            expand_prefixed("https://example.com/foo", &prefixes),
            Some("https://example.com/foo".to_string())
        );
    }

    #[test]
    fn expand_prefixed_unknown_prefix_returns_none() {
        let prefixes = vec![];
        assert_eq!(expand_prefixed("unknown:Foo", &prefixes), None);
    }
}
