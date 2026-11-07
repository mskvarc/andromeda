//! Reading one Rust source or manifest for the XSD datatype library.
//!
//! `xsd-rs` reads the lexical spaces of the XSD datatypes, whose `dateTime`,
//! `date` and `time` admit zone offsets and any fractional width. It is the
//! reader of a literal declared as an XSD datatype and of nothing else, so only
//! the lexicon's datatype module names it: an NGSI-LD time value read through it
//! would admit what clause 4.6.3 refuses.

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use syn::File;
use toml_edit::{DocumentMut, Item};

/// The crate name of the XSD datatype library in code.
const XSD_CRATE: &str = "xsd_rs";

/// The package name of the XSD datatype library in a manifest.
const XSD_PACKAGE: &str = "xsd-rs";

/// The dependency tables a manifest may declare the library in.
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// The line of every name of the library in `file`.
pub(crate) fn xsd_names(file: &File) -> Vec<usize> {
    let mut lines = Vec::new();
    read_tokens(file.to_token_stream(), &mut lines);
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// Read one token stream, descending into every group.
fn read_tokens(stream: TokenStream, lines: &mut Vec<usize>) {
    for token in stream {
        match token {
            TokenTree::Group(group) => read_tokens(group.stream(), lines),
            TokenTree::Ident(ident) if ident == XSD_CRATE => lines.push(ident.span().start().line),
            TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => {}
        }
    }
}

/// One finding per dependency table of `manifest` that declares the library.
pub(crate) fn xsd_dependency_findings(manifest: &str) -> Vec<String> {
    let Ok(document) = manifest.parse::<DocumentMut>() else {
        return Vec::new();
    };
    DEPENDENCY_TABLES
        .into_iter()
        .filter(|table| {
            [document.get(table), document.get("workspace").and_then(|workspace| workspace.get(table))]
                .into_iter()
                .flatten()
                .filter_map(Item::as_table_like)
                .any(|dependencies| dependencies.contains_key(XSD_PACKAGE))
        })
        .map(|table| format!("[{table}] depends on `{XSD_PACKAGE}`, which only the lexicon's datatype module reads"))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::time_handling::xsd_reading::{xsd_dependency_findings, xsd_names};

    #[test]
    fn a_name_of_the_xsd_library_is_found_in_code_and_in_macros() {
        let file = syn::parse_file("use xsd_rs::Datatype;\nfn f() { assert!(xsd_rs::Datatype::from_iri(&iri).is_some()); }").expect("a source");
        pretty_assertions::assert_eq!(xsd_names(&file), vec![1, 2]);
        let clean = syn::parse_file("fn f() { let xsd = 1; }").expect("a source");
        pretty_assertions::assert_eq!(xsd_names(&clean), Vec::<usize>::new());
    }

    #[test]
    fn a_dependency_on_the_xsd_library_is_a_finding() {
        pretty_assertions::assert_eq!(xsd_dependency_findings("[dependencies]\nxsd-rs = \"0.10.0\"\n").len(), 1);
        pretty_assertions::assert_eq!(xsd_dependency_findings("[dependencies]\nchrono = \"0.4.45\"\n").len(), 0);
    }
}
