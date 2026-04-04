#![feature(proc_macro_span)]

//! Proc-macro crate that generates NGSI-LD vocabulary constants from local JSON-LD contexts.
//!
//! The macro reads three local context files at compile time, resolves prefixes and terms
//! offline, and emits typed `&'static iref::Iri` constants plus string lookup tables.

mod codegen;
mod model;
mod resolve;

use crate::{
    codegen::generate_tokens,
    model::{InputContexts, MacroInput},
    resolve::{load_contexts, resolve_call_site_base_dir},
};
use proc_macro::TokenStream;
use syn::parse_macro_input;

/// Generate NGSI-LD vocabulary constants from exactly three local JSON-LD context files.
///
/// The paths are resolved relative to the macro call site.
#[proc_macro]
pub fn generate(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as MacroInput);

    expand(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

fn expand(input: MacroInput) -> syn::Result<proc_macro2::TokenStream> {
    let base_dir = resolve_call_site_base_dir(&input.core)?;
    let contexts = InputContexts::resolve(base_dir, input)?;
    let resolved = load_contexts(&contexts)?;

    generate_tokens(&contexts, &resolved)
}

#[cfg(test)]
mod tests {
    #[test]
    fn ui() {
        let cases = trybuild::TestCases::new();
        cases.pass("tests/ui/pass/*.rs");
        cases.compile_fail("tests/ui/fail/*.rs");
    }
}
