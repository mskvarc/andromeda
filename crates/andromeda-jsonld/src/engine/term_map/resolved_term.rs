//! One term of an active context, after the context that declared it resolved it.
//!
//! A [`ResolvedTerm`] is what [`crate::engine::term_map::BatchTermMap`] is made
//! of: the IRI a term finally expands to, the coercion and container it
//! declares, and whether the fastpath may use it as a compact-IRI prefix. The
//! prefix question is a JSON-LD rule rather than a matter of taste, so it is
//! answered here once and read by everything that resolves a `prefix:suffix`
//! spelling.

use crate::engine::term_map::coercion::{TermCoercion, TermContainer};
use iri_rs::IriBuf;

/// The RFC 3986 `gen-delims` characters.
///
/// JSON-LD 1.1 clause 4.1.2 gives a simple term definition the prefix flag when
/// its IRI mapping ends with one of these, which is what makes
/// `"example": "http://example.org/"` a prefix and
/// `"speed": "http://example.org/speed"` an ordinary term.
const GEN_DELIMS: [char; 7] = [':', '/', '?', '#', '[', ']', '@'];

/// One resolved JSON-LD term declaration.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedTerm {
    /// Expanded term IRI.
    pub(crate) iri: IriBuf,
    /// Declared type coercion.
    pub(crate) coercion: Option<TermCoercion>,
    /// Declared value container.
    pub(crate) container: Option<TermContainer>,
    /// Whether the term carries a scoped context.
    pub(crate) has_scoped_context: bool,
    /// Whether this is a simple string declaration usable as a prefix.
    pub(crate) simple: bool,
}

impl ResolvedTerm {
    /// The namespace this term contributes when it is written as a compact-IRI
    /// prefix, or `None` when JSON-LD does not give it the prefix flag.
    ///
    /// Only a simple `term: "iri"` declaration whose IRI ends with a gen-delim
    /// carries the flag (JSON-LD 1.1 clause 4.1.2); an expanded (object)
    /// definition would need `@prefix: true`, which the fastpath does not
    /// model, so it is never treated as a prefix here.
    pub(crate) fn prefix_mapping(&self) -> Option<&IriBuf> {
        (self.simple && self.iri.ends_with(GEN_DELIMS)).then_some(&self.iri)
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::term_map::resolved_term::ResolvedTerm;
    use iri_rs::IriBuf;

    fn term(iri: &str, simple: bool) -> ResolvedTerm {
        ResolvedTerm {
            iri: IriBuf::new(iri.to_owned()).expect("a valid IRI"),
            coercion: None,
            container: None,
            has_scoped_context: false,
            simple,
        }
    }

    /// JSON-LD 1.1 clause 4.1.2: a simple declaration ending with a gen-delim
    /// carries the prefix flag. `/` and `#` are the two the vocabularies in use
    /// are written with, and `:` is what a URN namespace ends with.
    #[test]
    fn a_simple_declaration_ending_with_a_gen_delim_is_a_prefix() {
        for iri in ["http://example.org/", "https://purl.org/geojson/vocab#", "urn:example:"] {
            assert!(term(iri, true).prefix_mapping().is_some(), "{iri}");
        }
    }

    /// A simple declaration that names one thing rather than a namespace is not
    /// a prefix, so `speed:0` never becomes `http://example.org/speed0`.
    #[test]
    fn a_simple_declaration_that_names_a_single_term_is_not_a_prefix() {
        assert!(term("http://example.org/speed", true).prefix_mapping().is_none());
    }

    /// An expanded term definition needs `@prefix: true` to become one, which
    /// the fastpath does not model, so it never contributes a namespace.
    #[test]
    fn an_expanded_term_definition_is_never_a_prefix() {
        assert!(term("http://example.org/", false).prefix_mapping().is_none());
    }
}
