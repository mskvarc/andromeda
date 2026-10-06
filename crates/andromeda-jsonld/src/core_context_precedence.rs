//! Keeping the NGSI-LD Core @context authoritative and singular.
//!
//! Clauses 4.4 and 5.5.7 of ETSI GS CIM 009 v1.9.1 make the Core @context
//! protected, immutable, and possessed of "absolute precedence, regardless of
//! the position of the Core @context in the @context array". A broker realises
//! that by applying the Core @context exactly once, in the last (authoritative)
//! position. [`crate::document_context`] does this for the `@context` array it
//! assembles for each operation: it drops any core reference the request
//! carried, wherever the request placed it, then appends the canonical core
//! pair last.
//!
//! This module is the same policy one layer deeper. A context *document* the
//! loader fetches, or the operator preloads, may itself import the Core
//! @context — a user context written as `["<core-url>", { ...user terms... }]`.
//! The array-level policy above only ever sees the URL of such a document, not
//! the core reference nested inside it, so once the broker appends the
//! authoritative Core last, the document's embedded copy is an *earlier*,
//! protected occurrence. A conforming JSON-LD processor then rejects the later
//! authoritative copy as a `ProtectedTermRedefinition` instead of letting it
//! take precedence. Removing the embedded reference at load time is what keeps
//! the Core singular and last, so a user context that redefines a core term
//! resolves to the Core's definition — the Core wins, exactly as 5.5.7 requires
//! — rather than failing.
//!
//! This is the spec-mandated Core-precedence obligation, not a workaround: it is
//! how a broker built on an off-the-shelf JSON-LD processor meets clause 5.5.7.
//!
//! What the removal takes away is also what a response needs to know about the
//! document: whether it leaves the Core in force last (clause 4.4, Annex C.8).
//! That fact is read from the document as written and kept beside the
//! processed copy as its [`crate::context_tail::ContextTail`].
//!
//! The rules live here; the walk that applies them to a document the processor
//! reads is `engine::context_document`'s, and the one that applies them to the
//! contexts a request named is [`crate::context_value`]'s.

use crate::core_context::is_ngsi_ld_core_context_url;
use iri_rs::Iri;

/// The keyword JSON-LD 1.1 section 4.1.10 imports a context through.
pub(crate) const IMPORT_KEYWORD: &str = "@import";

/// The keywords that may stand beside `@import` in a definition that leaves
/// the imported context in force: `@version` states the processing mode and
/// `@protected` protects the definitions that follow, and neither defines a
/// term or changes an active-context setting (JSON-LD 1.1 section 9.15).
pub(crate) const IMPORT_ONLY_KEYWORDS: [&str; 3] = [IMPORT_KEYWORD, "@version", "@protected"];

/// Whether a context definition whose members are named `keys` defines
/// nothing but an import: every member is `@import`, `@version` or
/// `@protected`.
///
/// Such a definition that imported the Core defines nothing of its own once
/// the import is dropped, so it is emptied altogether: left as
/// `{"@version": 1.1}` it would stand as a context of its own after the import
/// is gone, and a response would have to host a wrapper for a definition that
/// says nothing. The same fact decides whether a definition leaves the context
/// it imports in force last.
pub(crate) fn defines_nothing_but_an_import<'k>(mut keys: impl Iterator<Item = &'k str>) -> bool {
    keys.all(|key| IMPORT_ONLY_KEYWORDS.contains(&key))
}

/// Whether a JSON string contains a validated NGSI-LD Core Context IRI.
pub(crate) fn is_core_context_json_string(value: &str) -> bool {
    Iri::parse(value).is_ok_and(is_ngsi_ld_core_context_url)
}

#[cfg(test)]
mod tests {
    use crate::{
        core_context::NGSI_LD_CORE_CONTEXT_V18_URL,
        core_context_precedence::{defines_nothing_but_an_import, is_core_context_json_string},
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use std::iter;

    #[test]
    fn every_release_of_the_core_context_is_a_core_reference() {
        assert!(is_core_context_json_string(NGSI_LD_CORE_CONTEXT_URL.as_str()));
        assert!(is_core_context_json_string(NGSI_LD_CORE_CONTEXT_V18_URL.as_str()));
        assert!(!is_core_context_json_string("https://example.com/context.jsonld"));
        assert!(!is_core_context_json_string("relative.jsonld"));
    }

    #[test]
    fn a_definition_of_version_and_protected_beside_its_import_defines_nothing_else() {
        assert!(defines_nothing_but_an_import(["@import", "@version", "@protected"].into_iter()));
        assert!(defines_nothing_but_an_import(iter::empty()));
        assert!(!defines_nothing_but_an_import(["@import", "speed"].into_iter()));
        assert!(!defines_nothing_but_an_import(["@import", "@vocab"].into_iter()));
    }
}
