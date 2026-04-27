// The workspace-wide `clippy.toml` bans direct `Value` inspection for the
// repository layer in the main `andromeda` crate; this proc-macro crate uses
// `Value::get` etc. legitimately on externally-supplied JSON-LD contexts.
#![allow(clippy::disallowed_methods)]

//! Proc-macro crate that generates NGSI-LD vocabulary constants from local JSON-LD contexts.
//!
//! The macro reads two local context files at compile time, resolves prefixes and terms
//! offline, and emits typed `&'static iref::Iri` constants plus string lookup tables.

mod codegen;
mod model;
mod resolve;

use crate::{
    codegen::generate_tokens,
    model::{InputContexts, MacroInput},
    resolve::{load_contexts, resolve_workspace_root},
};
use proc_macro::TokenStream;
use syn::parse_macro_input;

/// Generate NGSI-LD vocabulary constants from exactly two local JSON-LD context files.
///
/// Paths are resolved as absolute paths from the workspace root.
#[proc_macro]
pub fn generate(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as MacroInput);

    expand(input).unwrap_or_else(syn::Error::into_compile_error).into()
}

fn expand(input: MacroInput) -> syn::Result<proc_macro2::TokenStream> {
    let workspace_root = resolve_workspace_root(&input.core)?;
    let contexts = InputContexts::resolve(workspace_root, input)?;
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
