use crate::model::ResolvedModel;
use proc_macro2::{Literal, Span, TokenStream};
use quote::quote;
use syn::Ident;

/// Generate the final Rust token stream for the resolved vocabulary model.
pub fn generate_tokens(inputs: &crate::model::InputContexts, model: &ResolvedModel) -> syn::Result<TokenStream> {
    let include_paths = model
        .include_paths
        .iter()
        .map(|path| {
            quote! {
                const _: &str = include_str!(#path);
            }
        })
        .collect::<Vec<_>>();

    let prefix_consts = model
        .prefixes
        .iter()
        .map(|prefix| {
            let ident = Ident::new(&prefix.const_name, Span::call_site());
            let expanded = Literal::string(&prefix.expanded);
            let doc = format!("`{}` -> `{}`", prefix.compact, prefix.expanded);
            quote! {
                #[doc = #doc]
                pub const #ident: &'static iref::Iri = static_iref::iri!(#expanded);
            }
        })
        .collect::<Vec<_>>();

    let expanded_term_consts = model
        .terms
        .iter()
        .map(|term| {
            let ident = Ident::new(&term.const_name, Span::call_site());
            let expanded = Literal::string(&term.expanded);
            let doc = format!("`{}` -> `{}`", term.compact, term.expanded);
            quote! {
                #[doc = #doc]
                pub const #ident: &'static iref::Iri = static_iref::iri!(#expanded);
            }
        })
        .collect::<Vec<_>>();

    let compact_term_consts = model
        .terms
        .iter()
        .map(|term| {
            let ident = Ident::new(&term.const_name, Span::call_site());
            let compact = Literal::string(&term.compact);
            let doc = format!("`{}`", term.compact);
            quote! {
                #[doc = #doc]
                pub const #ident: &'static str = #compact;
            }
        })
        .collect::<Vec<_>>();

    let term_count = model.terms.len();

    let compact_entries = model
        .terms
        .iter()
        .map(|term| (term.compact.as_str(), term.expanded.as_str()))
        .collect::<Vec<_>>();
    let compact_to_expanded = compact_entries
        .iter()
        .map(|(compact, expanded)| quote! { (#compact, #expanded) })
        .collect::<Vec<_>>();

    let mut expanded_entries = model
        .terms
        .iter()
        .map(|term| (term.expanded.as_str(), term.compact.as_str()))
        .collect::<Vec<_>>();
    expanded_entries.sort_by_key(|(expanded, _)| *expanded);
    let expanded_to_compact = expanded_entries
        .iter()
        .map(|(expanded, compact)| quote! { (#expanded, #compact) })
        .collect::<Vec<_>>();

    let _ = inputs;

    Ok(quote! {
        #(#include_paths)*

        /// Namespace prefix constants.
        pub mod prefix {
            #(#prefix_consts)*
        }

        /// Expanded IRI constants for all context-derived terms.
        pub mod expanded {
            #(#expanded_term_consts)*
        }

        /// Compact term name constants for all context-derived terms.
        pub mod compact {
            #(#compact_term_consts)*
        }

        /// Total number of context-derived terms.
        pub const TERM_COUNT: usize = #term_count;

        /// (compact_name, expanded_iri) pairs, sorted by compact name.
        pub const COMPACT_TO_EXPANDED: &[(&str, &str)] = &[
            #(#compact_to_expanded),*
        ];

        /// (expanded_iri, compact_name) pairs, sorted by expanded IRI.
        pub const EXPANDED_TO_COMPACT: &[(&str, &str)] = &[
            #(#expanded_to_compact),*
        ];

        /// Look up expanded IRI by compact name via binary search.
        pub fn compact_to_expanded(compact: &str) -> Option<&'static str> {
            COMPACT_TO_EXPANDED
                .binary_search_by_key(&compact, |(candidate, _)| candidate)
                .ok()
                .map(|index| COMPACT_TO_EXPANDED[index].1)
        }

        /// Look up compact name by expanded IRI via binary search.
        pub fn expanded_to_compact(expanded: &str) -> Option<&'static str> {
            EXPANDED_TO_COMPACT
                .binary_search_by_key(&expanded, |(candidate, _)| candidate)
                .ok()
                .map(|index| EXPANDED_TO_COMPACT[index].1)
        }
    })
}
