//! The IRI a `@context` writes for a term, and how the active context resolves it.
//!
//! JSON-LD 1.1 clause 4.1.2 ("Create Term Definition") IRI-expands a term
//! definition's `@id` against the active context, and clause 4.1.3 ("IRI
//! Expansion") resolves a `prefix:suffix` spelling through the term definition
//! the prefix names. `{"example": "http://example.org/", "Place":
//! "example:Place"}` therefore defines `Place` as `http://example.org/Place`.
//!
//! Keeping the declared text as written until that resolution can run is the
//! whole point of this module. A compact IRI is syntactically indistinguishable
//! from an absolute IRI under an unregistered scheme -- `example:Place` parses
//! as scheme `example`, path `Place` -- so a parse that validated the text into
//! an `IriBuf` on sight recorded the compact spelling as if it were already
//! expanded, and the batch fastpath then stored entities under term IRIs the
//! JSON-LD processor never produces.
//!
//! The resolution is on demand rather than in declaration order, because a
//! JSON object states no order and clause 4.1.2 defines the prefix's own term
//! before using it: `{"Place": "example:Place", "example": "http://example.org/"}`
//! resolves exactly as the reverse spelling does.

use crate::engine::{
    context_url::{is_absolute_iri, is_full_iri},
    term_map::{resolved_term::ResolvedTerm, user_term::DeclaredTerm},
};
use iri_rs::{InvalidIri, IriBuf};
use std::collections::{HashMap, HashSet};
use thiserror::Error;

/// The IRI text one term declaration wrote, before any context resolved it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeclaredIri(Box<str>);

impl DeclaredIri {
    /// Keep the text a `@context` wrote for one term.
    pub(crate) fn new(declared: &str) -> DeclaredIri {
        DeclaredIri(Box::<str>::from(declared))
    }

    /// The text exactly as the context wrote it.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// A term declaration whose IRI the fastpath cannot resolve to one unambiguous
/// expansion.
///
/// Every such context is handed to the JSON-LD processor instead, which is what
/// keeps a shape the fastpath does not model from expanding differently on the
/// two write paths.
#[derive(Debug, Error)]
pub(crate) enum UnresolvableTermIri {
    /// The declaration reaches itself through a chain of prefixes: the "cyclic
    /// IRI mapping" of clause 4.1.2.
    #[error("a term declaration reaches itself through a chain of prefixes")]
    CyclicPrefix,
    /// The declaration names neither an absolute IRI nor a compact IRI whose
    /// prefix the active context defines.
    #[error("a term declaration names neither an absolute IRI nor a defined prefix")]
    NoExpansion,
    /// The declaration's expansion does not parse as an IRI.
    #[error("a term declaration expands to a malformed IRI")]
    Malformed(#[source] InvalidIri<String>),
}

/// Resolve every declaration one parsed context document makes against the
/// terms already active.
///
/// `active` is the table accumulated so far -- the core context's terms plus
/// whatever earlier contexts of the same request declared -- and the returned
/// table is this document's contribution to it. A document-local declaration
/// shadows an active one of the same name while the document is being resolved,
/// which is the order clause 4.1.2 processes a context in.
///
/// # Errors
///
/// Returns [`UnresolvableTermIri`] for the first declaration that names neither
/// an absolute IRI nor a prefix the active context defines, and for one that
/// reaches itself through a chain of prefixes -- the "cyclic IRI mapping" of
/// clause 4.1.2, which the fastpath declines rather than diagnoses.
pub(crate) fn resolve_term_declarations(
    declared: &HashMap<Box<str>, DeclaredTerm>,
    active: &HashMap<Box<str>, ResolvedTerm>,
) -> Result<HashMap<Box<str>, ResolvedTerm>, UnresolvableTermIri> {
    let mut resolved: HashMap<Box<str>, ResolvedTerm> = HashMap::with_capacity(declared.len());
    let mut open: HashSet<Box<str>> = HashSet::new();
    for term in declared.keys() {
        resolve_one(term, declared, active, &mut resolved, &mut open)?;
    }
    Ok(resolved)
}

/// Resolve one declared term, resolving whatever prefix it names first.
fn resolve_one(
    term: &str,
    declared: &HashMap<Box<str>, DeclaredTerm>,
    active: &HashMap<Box<str>, ResolvedTerm>,
    resolved: &mut HashMap<Box<str>, ResolvedTerm>,
    open: &mut HashSet<Box<str>>,
) -> Result<(), UnresolvableTermIri> {
    if resolved.contains_key(term) {
        return Ok(());
    }
    let Some(declaration) = declared.get(term) else {
        // Not declared by this document: whatever the active context already
        // says about the name stands, and a name nothing declares is left to
        // the `@vocab` fallback at expansion time.
        return Ok(());
    };
    if !open.insert(Box::<str>::from(term)) {
        return Err(UnresolvableTermIri::CyclicPrefix);
    }
    let expansion = expand_declaration(declaration, declared, active, resolved, open);
    open.remove(term);
    let iri = expansion?;
    resolved.insert(
        Box::<str>::from(term),
        ResolvedTerm {
            iri,
            coercion: declaration.coercion.clone(), // clone: the parsed document is cached and outlives this table
            container: declaration.container,
            has_scoped_context: declaration.has_scoped_context,
            simple: declaration.simple,
        },
    );
    Ok(())
}

/// Expand one declaration's IRI text the way clause 4.1.3 expands it.
fn expand_declaration(
    declaration: &DeclaredTerm,
    declared: &HashMap<Box<str>, DeclaredTerm>,
    active: &HashMap<Box<str>, ResolvedTerm>,
    resolved: &mut HashMap<Box<str>, ResolvedTerm>,
    open: &mut HashSet<Box<str>>,
) -> Result<IriBuf, UnresolvableTermIri> {
    let text = declaration.iri.as_str();
    if is_full_iri(text) {
        return iri_of(text);
    }
    if let Some((prefix, suffix)) = text.split_once(':') {
        // Clause 4.1.3: `_` opens a blank node identifier and `//` opens an
        // authority component, so neither spelling is a compact IRI to resolve
        // through a prefix.
        if prefix != "_" && !suffix.starts_with("//") {
            resolve_one(prefix, declared, active, resolved, open)?;
            if let Some(namespace) = prefix_namespace(prefix, active, resolved) {
                return iri_of(&format!("{namespace}{suffix}"));
            }
        }
        // No context defines the prefix, so the text is already an absolute IRI
        // and stands as written -- the reading the processor gives
        // `did:example:1`.
        if is_absolute_iri(text) {
            return iri_of(text);
        }
    }
    Err(UnresolvableTermIri::NoExpansion)
}

/// The namespace a prefix names, taking this document's own declarations ahead
/// of the terms that were already active.
///
/// The built-in prefixes of [`crate::engine::term_map`] are deliberately not
/// consulted: they include `xsd`, which neither the core context nor any user
/// context declares, so resolving a declaration through them would make the
/// fastpath expand `{"T": "xsd:Thing"}` to a different IRI than the processor
/// does -- the very divergence this resolution exists to close.
fn prefix_namespace<'a>(prefix: &str, active: &'a HashMap<Box<str>, ResolvedTerm>, resolved: &'a HashMap<Box<str>, ResolvedTerm>) -> Option<&'a IriBuf> {
    resolved.get(prefix).or_else(|| active.get(prefix)).and_then(ResolvedTerm::prefix_mapping)
}

/// Validate one expansion as an IRI.
fn iri_of(expansion: &str) -> Result<IriBuf, UnresolvableTermIri> {
    IriBuf::new(expansion.to_owned()).map_err(UnresolvableTermIri::Malformed)
}

#[cfg(test)]
mod tests {
    use crate::engine::term_map::{
        declared_iri::{DeclaredIri, UnresolvableTermIri, resolve_term_declarations},
        resolved_term::ResolvedTerm,
        user_term::DeclaredTerm,
    };
    use iri_rs::IriBuf;
    use std::collections::HashMap;

    /// A simple `term: "iri"` declaration.
    fn simple(iri: &str) -> DeclaredTerm {
        DeclaredTerm {
            iri: DeclaredIri::new(iri),
            coercion: None,
            container: None,
            has_scoped_context: false,
            simple: true,
        }
    }

    /// An expanded `term: {"@id": "iri"}` declaration.
    fn expanded(iri: &str) -> DeclaredTerm {
        DeclaredTerm { simple: false, ..simple(iri) }
    }

    fn document(entries: &[(&str, DeclaredTerm)]) -> HashMap<Box<str>, DeclaredTerm> {
        entries
            .iter()
            .map(|(term, declaration)| (Box::<str>::from(*term), declaration.clone()))
            .collect()
    }

    fn active(entries: &[(&str, &str)]) -> HashMap<Box<str>, ResolvedTerm> {
        entries
            .iter()
            .map(|(term, iri)| {
                (
                    Box::<str>::from(*term),
                    ResolvedTerm {
                        iri: IriBuf::new((*iri).to_owned()).expect("a valid IRI"),
                        coercion: None,
                        container: None,
                        has_scoped_context: false,
                        simple: true,
                    },
                )
            })
            .collect()
    }

    fn expansion_of(resolved: &HashMap<Box<str>, ResolvedTerm>, term: &str) -> String {
        resolved.get(term).expect("the term resolved").iri.as_str().to_owned()
    }

    /// The defect this module exists for: a term whose `@id` is a compact IRI
    /// resolves through the prefix its own context declares.
    #[test]
    fn a_term_declared_by_a_compact_iri_resolves_through_its_own_contexts_prefix() {
        let declared = document(&[("example", simple("http://example.org/")), ("Place", simple("example:Place"))]);

        let resolved = resolve_term_declarations(&declared, &HashMap::new()).expect("both terms resolve");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "http://example.org/Place");
    }

    /// A JSON object states no member order, so the prefix is defined on demand
    /// rather than in iteration order (clause 4.1.2).
    #[test]
    fn a_prefix_declared_after_the_term_that_uses_it_still_resolves() {
        let declared = document(&[("Place", expanded("example:Place")), ("example", simple("http://example.org/"))]);

        let resolved = resolve_term_declarations(&declared, &HashMap::new()).expect("both terms resolve");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "http://example.org/Place");
    }

    /// A prefix may itself be declared by a compact IRI, so the chain is
    /// followed to the end.
    #[test]
    fn a_chain_of_prefixes_resolves_to_the_iri_at_its_root() {
        let declared = document(&[
            ("base", simple("http://example.org/")),
            ("sub", simple("base:ontology/")),
            ("Place", simple("sub:Place")),
        ]);

        let resolved = resolve_term_declarations(&declared, &HashMap::new()).expect("the chain resolves");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "http://example.org/ontology/Place");
    }

    /// A term whose `@id` is already an absolute IRI is kept exactly as written.
    #[test]
    fn a_term_declared_by_an_absolute_iri_is_kept_verbatim() {
        let declared = document(&[("Place", simple("http://example.org/Place")), ("id", simple("urn:example:id"))]);

        let resolved = resolve_term_declarations(&declared, &HashMap::new()).expect("both terms resolve");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "http://example.org/Place");
        pretty_assertions::assert_eq!(expansion_of(&resolved, "id"), "urn:example:id");
    }

    /// Clause 4.1.3: with no term definition for the prefix the text is already
    /// an absolute IRI, so it stands rather than being invented into one.
    #[test]
    fn a_term_whose_prefix_no_context_declares_stays_the_absolute_iri_it_is() {
        let declared = document(&[("Place", simple("unknown:Place"))]);

        let resolved = resolve_term_declarations(&declared, &HashMap::new()).expect("the term resolves");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "unknown:Place");
    }

    /// A prefix an earlier context of the same request declared is active for
    /// the later one.
    #[test]
    fn a_prefix_declared_by_an_earlier_context_resolves_a_later_documents_term() {
        let declared = document(&[("Place", simple("example:Place"))]);

        let resolved = resolve_term_declarations(&declared, &active(&[("example", "http://example.org/")])).expect("the term resolves");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "http://example.org/Place");
    }

    /// A document's own declaration of a prefix outranks the active one while
    /// that document is being resolved.
    #[test]
    fn a_document_local_prefix_shadows_the_active_one_of_the_same_name() {
        let declared = document(&[("example", simple("http://later.example/")), ("Place", simple("example:Place"))]);

        let resolved = resolve_term_declarations(&declared, &active(&[("example", "http://earlier.example/")])).expect("both terms resolve");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "http://later.example/Place");
    }

    /// An expanded term definition carries no prefix flag, so a term written
    /// against it is not a compact IRI and stays the absolute IRI it is.
    #[test]
    fn an_expanded_prefix_definition_does_not_resolve_a_compact_iri() {
        let declared = document(&[("example", expanded("http://example.org/")), ("Place", simple("example:Place"))]);

        let resolved = resolve_term_declarations(&declared, &HashMap::new()).expect("both terms resolve");

        pretty_assertions::assert_eq!(expansion_of(&resolved, "Place"), "example:Place");
    }

    /// A declaration that is neither an absolute IRI nor a compact one against
    /// a declared prefix is refused, so the context takes the processor's path.
    #[test]
    fn a_declaration_that_names_no_iri_at_all_is_refused() {
        let declared = document(&[("Place", simple("Place"))]);

        let error = resolve_term_declarations(&declared, &HashMap::new()).expect_err("a bare term names no IRI");

        assert!(matches!(error, UnresolvableTermIri::NoExpansion), "{error}");
    }

    /// Clause 4.1.2's cyclic IRI mapping: two terms that name each other as a
    /// prefix resolve to nothing, and the context is declined.
    #[test]
    fn a_cycle_between_two_prefix_declarations_is_refused() {
        let declared = document(&[("alpha", simple("beta:a/")), ("beta", simple("alpha:b/"))]);

        let error = resolve_term_declarations(&declared, &HashMap::new()).expect_err("a prefix cycle resolves to nothing");

        assert!(matches!(error, UnresolvableTermIri::CyclicPrefix), "{error}");
    }

    /// A blank node identifier is not a compact IRI, and it is not an absolute
    /// IRI either, so the context is declined rather than mis-expanded.
    #[test]
    fn a_blank_node_declaration_is_refused_rather_than_resolved_through_a_prefix() {
        let declared = document(&[("_", simple("http://example.org/")), ("Place", simple("_:Place"))]);

        let error = resolve_term_declarations(&declared, &HashMap::new()).expect_err("a blank node identifier is not a term IRI");

        assert!(matches!(error, UnresolvableTermIri::NoExpansion), "{error}");
    }
}
