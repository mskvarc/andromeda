//! What a `@context` leaves in force last.
//!
//! JSON-LD processes the entries of a `@context` in order, and a later entry
//! overrides what an earlier one set (JSON-LD 1.1 section 4.1). Clause 4.4 asks
//! a response to leave the NGSI-LD core context in force last, "so that [...]
//! upstream JSON-LD processors can properly expand as NGSI-LD", and Annex C.8
//! shows a broker answering with a client's wrapper `@context` alone when that
//! wrapper already ends with the core context. Deciding whether a response
//! still owes the core context therefore needs one fact about each context a
//! client names: what the last context it processes is.
//!
//! [`ContextTail`] is that fact, read from a document as written, before the
//! broker drops the document's own core reference for processing
//! ([`crate::core_context_precedence`]), since that reference is exactly what
//! the fact records. A relative reference is kept as written until the URL the
//! document is processed under is known, because that URL, not the key the
//! document happens to be stored under, is what JSON-LD resolves it against.

use crate::{
    context_value::ContextEntry,
    core_context::is_ngsi_ld_core_context_url,
    core_context_precedence::defines_nothing_but_an_import,
    engine::context_url::is_absolute_iri,
    hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
};
use andromeda_json_literal::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_vocabulary::{NGSI_LD_CORE_CONTEXT_URL, keyword};
use iri_rs::{Iri, IriBuf, IriRefBuf};

/// The last context a `@context` value leaves in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextTail {
    /// The NGSI-LD core context this broker implements, v1.9 (clause 4.4).
    CurrentCore,
    /// Another release of the core context, Version 1.0 under the versionless
    /// URL included, whose published document differs from v1.9.
    OtherCore,
    /// Whatever the referenced context document leaves in force last.
    Reference(IriBuf),
    /// A relative reference, as written: whatever the document it resolves to,
    /// against the URL of the document it was read from, leaves in force last
    /// (JSON-LD 1.1 API section 4.1.2 step 5.2.1).
    Relative(IriRefBuf),
    /// Term definitions written in place, a context nullification, or no
    /// context at all.
    LocalDefinitions,
    /// A reference this broker cannot follow: a string that is no IRI
    /// reference, a relative reference with no document to resolve it against,
    /// or one that does not resolve.
    Unresolvable,
}

impl ContextTail {
    /// The tail of a context document, `{"@context": ...}`, as written.
    #[must_use]
    pub fn of_document(document: JsonPiece<'_>) -> ContextTail {
        match document.member("@context") {
            Some(context) => ContextTail::of_context(context),
            None => ContextTail::LocalDefinitions,
        }
    }

    /// The tail of a `@context` value: a reference, a definition, or an array
    /// of them processed in order.
    #[must_use]
    pub fn of_context(context: JsonPiece<'_>) -> ContextTail {
        match context.kind() {
            JsonKind::String => context
                .as_str()
                .map_or(ContextTail::Unresolvable, |reference| ContextTail::of_reference(&reference)),
            JsonKind::Array => context
                .elements()
                .and_then(|entries| entries.last().copied())
                .map_or(ContextTail::LocalDefinitions, ContextTail::of_context),
            JsonKind::Object => ContextTail::of_definition(context),
            JsonKind::Null => ContextTail::LocalDefinitions,
            JsonKind::Boolean | JsonKind::Number => ContextTail::Unresolvable,
        }
    }

    /// The tail of one context a request named, which has no document URL to
    /// resolve a relative reference against.
    ///
    /// A relative reference to one of the tenant's own stored `@context`s is
    /// read as the identifier `hosted` recognises it by, exactly as processing
    /// reads it, so the tail follows the stored document.
    /// Any other relative reference cannot be followed. Any other context is
    /// read as [`Self::of_context`] reads it; an absolute reference to a stored
    /// context is the loader's to recognise when it is followed.
    #[must_use]
    pub fn of_request_context(context: &ContextEntry<'_>, hosted: &dyn HostedContextRecognizer) -> ContextTail {
        match context {
            ContextEntry::Reference(reference) if !is_absolute_iri(reference) => match IriRefBuf::new(reference.as_ref()) {
                Ok(relative) => match hosted.recognize(&relative) {
                    HostedReference::Own(identifier) => match identifier.to_iri() {
                        Ok(stored) => ContextTail::Reference(stored),
                        // A URN is an IRI, so this is never reached; if it were,
                        // the reference could not be followed and the core
                        // context is appended, which a response always accepts.
                        Err(_not_an_iri) => ContextTail::Unresolvable,
                    },
                    HostedReference::Unrelated => ContextTail::Unresolvable,
                },
                Err(_not_a_reference) => ContextTail::Unresolvable,
            },
            ContextEntry::Reference(reference) => ContextTail::of_reference(reference),
            ContextEntry::Definition(definition) => ContextTail::of_definition(definition.written()),
            ContextEntry::Null => ContextTail::LocalDefinitions,
            ContextEntry::Malformed(_) => ContextTail::Unresolvable,
        }
    }

    /// The tail of one context reference, as written.
    ///
    /// A core context URL is classified by name and never looked up: every
    /// release is embedded, and which release the reference names is the whole
    /// fact. Any other absolute reference is kept as the URL it is; whether
    /// that URL names one of this broker's own stored contexts is the loader's
    /// to recognise when the reference is followed. A relative reference is
    /// kept as written until [`Self::resolved_against`] resolves it.
    #[must_use]
    pub fn of_reference(reference: &str) -> ContextTail {
        match Iri::parse(reference) {
            Ok(absolute) => ContextTail::of_url(IriBuf::from(absolute)),
            Err(_not_absolute) => IriRefBuf::new(reference).map_or(ContextTail::Unresolvable, ContextTail::Relative),
        }
    }

    /// The tail of an absolute context URL.
    #[must_use]
    pub fn of_url(url: IriBuf) -> ContextTail {
        if url.as_str() == NGSI_LD_CORE_CONTEXT_URL.as_str() {
            ContextTail::CurrentCore
        } else if is_ngsi_ld_core_context_url(url.as_ref()) {
            ContextTail::OtherCore
        } else {
            ContextTail::Reference(url)
        }
    }

    /// This tail once a relative reference is resolved against `base`, the URL
    /// of the document it was read from; any other tail is unchanged.
    #[must_use]
    pub fn resolved_against(self, base: &IriBuf) -> ContextTail {
        match self {
            ContextTail::Relative(relative) => match relative.resolved(base) {
                Ok(resolved) => ContextTail::of_reference(resolved.as_str()),
                Err(_unresolvable) => ContextTail::Unresolvable,
            },
            other @ (ContextTail::CurrentCore
            | ContextTail::OtherCore
            | ContextTail::Reference(_)
            | ContextTail::LocalDefinitions
            | ContextTail::Unresolvable) => other,
        }
    }

    /// The tail of a context definition object.
    ///
    /// JSON-LD 1.1 section 4.1.10 merges an `@import`ed context into the
    /// definition that imports it, the definition's own terms taking
    /// precedence; only a definition that imports and otherwise sets nothing
    /// but `@version` and `@protected` leaves the imported context in force
    /// last. A term, `@vocab`, `@base`, `@language` or `@direction` beside the
    /// import is the definition's own and is in force after it.
    fn of_definition(definition: JsonPiece<'_>) -> ContextTail {
        let members = definition.members().unwrap_or_default();
        let imported = members
            .iter()
            .rev()
            .find_map(|(key, value)| (key == keyword::IMPORT).then_some(*value))
            .and_then(JsonPiece::as_str);
        match imported {
            Some(imported) if defines_nothing_but_an_import(members.iter().map(|(key, _)| key.as_ref())) => ContextTail::of_reference(&imported),
            Some(_) | None => ContextTail::LocalDefinitions,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_tail::ContextTail,
        context_value::ContextEntry,
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference, NoHostedContexts},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
    use andromeda_ngsi_ld_vocabulary::{
        NGSI_LD_CORE_CONTEXT_URL,
        core_context::{NGSI_LD_CORE_CONTEXT_V10_URL, NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V18_URL},
    };
    use iri_rs::{IriBuf, IriRefBuf};
    use serde_json::json;

    fn url(value: &str) -> IriBuf {
        IriBuf::new(value.to_owned()).expect("a valid URL")
    }

    fn relative(value: &str) -> IriRefBuf {
        IriRefBuf::new(value.to_owned()).expect("a valid IRI reference")
    }

    /// `value` as an admitted literal.
    fn literal(value: &serde_json::Value) -> JsonLiteral {
        JsonLiteral::admit(&value.to_string()).expect("an admissible document")
    }

    fn of_document(document: &serde_json::Value) -> ContextTail {
        ContextTail::of_document(literal(document).piece())
    }

    fn of_context(context: &serde_json::Value) -> ContextTail {
        ContextTail::of_context(literal(context).piece())
    }

    fn of_request_context(context: &serde_json::Value, hosted: &dyn HostedContextRecognizer) -> ContextTail {
        let context = literal(context);
        ContextTail::of_request_context(&ContextEntry::of(context.piece()), hosted)
    }

    /// The stored identifier the recogniser below knows.
    const STORED: &str = "urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";

    /// The stored path of [`STORED`].
    const STORED_PATH: &str = "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";

    /// Recognises the stored path of [`STORED`], root-relative or on any
    /// origin.
    #[derive(Debug)]
    struct StoredPath;

    impl HostedContextRecognizer for StoredPath {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            if reference.as_str().ends_with(STORED_PATH) {
                HostedReference::Own(STORED.parse::<JsonLdContextId>().expect("a stored identifier"))
            } else {
                HostedReference::Unrelated
            }
        }
    }

    /// A request has no base URL, so a root-relative reference to a stored
    /// context is followed through the identifier the tenant stores it under.
    #[test]
    fn a_root_relative_request_reference_to_a_stored_context_follows_its_identifier() {
        pretty_assertions::assert_eq!(of_request_context(&json!(STORED_PATH), &StoredPath), ContextTail::Reference(url(STORED)));
        pretty_assertions::assert_eq!(of_request_context(&json!(STORED_PATH), &NoHostedContexts), ContextTail::Unresolvable);
    }

    /// An absolute reference stays the URL it is: the loader recognises it
    /// when it is followed, and serves it from storage only when it is held.
    #[test]
    fn an_absolute_request_reference_stays_the_url_it_is() {
        let absolute = format!("http://127.0.0.1:1056{STORED_PATH}");
        pretty_assertions::assert_eq!(of_request_context(&json!(absolute), &StoredPath), ContextTail::Reference(url(&absolute)));
        pretty_assertions::assert_eq!(
            of_request_context(&json!(NGSI_LD_CORE_CONTEXT_URL.as_str()), &StoredPath),
            ContextTail::CurrentCore
        );
        pretty_assertions::assert_eq!(
            of_request_context(&json!({ "a": "https://example.com/a" }), &StoredPath),
            ContextTail::LocalDefinitions
        );
    }

    #[test]
    fn a_document_ending_with_the_v1_9_core_ends_with_the_current_core() {
        let compound = json!({ "@context": ["ngsi-ld-test-suite.jsonld", NGSI_LD_CORE_CONTEXT_URL.as_str()] });

        pretty_assertions::assert_eq!(of_document(&compound), ContextTail::CurrentCore);
    }

    #[test]
    fn a_document_ending_with_an_older_core_ends_with_another_core() {
        for core in [NGSI_LD_CORE_CONTEXT_V13_URL, NGSI_LD_CORE_CONTEXT_V18_URL, NGSI_LD_CORE_CONTEXT_V10_URL] {
            let document = json!({ "@context": [{ "a": "https://example.com/a" }, core.as_str()] });

            pretty_assertions::assert_eq!(of_document(&document), ContextTail::OtherCore, "{core}");
        }
    }

    #[test]
    fn a_document_importing_the_core_first_then_defining_terms_ends_with_its_own_definitions() {
        let document = json!({ "@context": [NGSI_LD_CORE_CONTEXT_URL.as_str(), { "@vocab": "https://example.com/" }] });

        pretty_assertions::assert_eq!(of_document(&document), ContextTail::LocalDefinitions);
    }

    #[test]
    fn a_document_ending_with_a_reference_defers_to_the_referenced_document() {
        let document = json!({ "@context": [{ "a": "https://example.com/a" }, "https://example.com/next.jsonld"] });

        pretty_assertions::assert_eq!(of_document(&document), ContextTail::Reference(url("https://example.com/next.jsonld")));
    }

    /// JSON-LD 1.1 API section 4.1.2: a relative reference inside a remote
    /// context resolves against that document's own URL, which is known only
    /// once the document is processed, so the reference is kept as written
    /// until then.
    #[test]
    fn a_relative_reference_is_kept_until_it_resolves_against_the_document_it_was_read_from() {
        let tail = of_document(&json!({ "@context": "next.jsonld" }));

        pretty_assertions::assert_eq!(tail, ContextTail::Relative(relative("next.jsonld")));
        pretty_assertions::assert_eq!(
            tail.resolved_against(&url("https://example.com/contexts/wrapper.jsonld")),
            ContextTail::Reference(url("https://example.com/contexts/next.jsonld"))
        );
    }

    /// A relative reference that resolves to the core context is the core.
    #[test]
    fn a_relative_reference_resolving_to_the_core_context_is_the_core() {
        let tail = ContextTail::of_reference("ngsi-ld-core-context-v1.9.jsonld");

        pretty_assertions::assert_eq!(
            tail.resolved_against(&url("https://uri.etsi.org/ngsi-ld/v1/wrapper.jsonld")),
            ContextTail::CurrentCore
        );
    }

    /// Resolution leaves every other tail as it is.
    #[test]
    fn resolving_leaves_an_absolute_tail_unchanged() {
        let base = url("https://example.com/base.jsonld");
        for tail in [
            ContextTail::CurrentCore,
            ContextTail::OtherCore,
            ContextTail::Reference(url("https://example.com/x.jsonld")),
            ContextTail::LocalDefinitions,
            ContextTail::Unresolvable,
        ] {
            pretty_assertions::assert_eq!(tail.clone().resolved_against(&base), tail); // clone: the expectation compares against it
        }
    }

    /// JSON-LD 1.1 section 4.1.10: a definition that only imports leaves the
    /// imported context in force; one that also defines terms overrides it.
    #[test]
    fn an_import_leaves_the_imported_context_in_force_only_when_nothing_else_is_defined() {
        pretty_assertions::assert_eq!(
            of_context(&json!({ "@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "@version": 1.1 })),
            ContextTail::CurrentCore
        );
        pretty_assertions::assert_eq!(
            of_context(&json!({ "@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "a": "https://example.com/a" })),
            ContextTail::LocalDefinitions
        );
        pretty_assertions::assert_eq!(of_context(&json!({ "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str() })), ContextTail::OtherCore);
    }

    /// A hosted context's root-relative path is a relative reference like any
    /// other: which absolute URL names one of this broker's own contexts is
    /// the loader's to recognise once it is resolved.
    #[test]
    fn a_hosted_context_path_is_a_relative_reference_like_any_other() {
        let tail = ContextTail::of_reference(STORED_PATH);

        pretty_assertions::assert_eq!(tail, ContextTail::Relative(relative(STORED_PATH)));
        pretty_assertions::assert_eq!(
            tail.resolved_against(&url("https://broker.example.com/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:outer")),
            ContextTail::Reference(url(&format!("https://broker.example.com{STORED_PATH}")))
        );
    }

    /// `@protected` defines no term and changes no active-context setting, so
    /// it may stand beside an `@import` that leaves the import in force; a
    /// `@vocab` beside it is the definition's own and is in force after it.
    #[test]
    fn protected_beside_an_import_leaves_the_import_in_force_and_vocab_does_not() {
        pretty_assertions::assert_eq!(
            of_context(&json!({ "@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "@protected": true })),
            ContextTail::CurrentCore
        );
        pretty_assertions::assert_eq!(
            of_context(&json!({ "@import": NGSI_LD_CORE_CONTEXT_URL.as_str(), "@protected": true, "@vocab": "https://example.com/" })),
            ContextTail::LocalDefinitions
        );
    }

    #[test]
    fn a_nullification_or_an_empty_array_leaves_no_core_in_force() {
        pretty_assertions::assert_eq!(of_context(&json!(null)), ContextTail::LocalDefinitions);
        pretty_assertions::assert_eq!(of_context(&json!([])), ContextTail::LocalDefinitions);
        pretty_assertions::assert_eq!(of_document(&json!({})), ContextTail::LocalDefinitions);
    }
}
