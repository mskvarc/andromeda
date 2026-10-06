//! Turning a [`ContextSpec`] into the `@context` a document is processed under.
//!
//! Clause 5.5.7 gives the core context absolute precedence: it is appended
//! *last* so that, under JSON-LD 1.1's rule of later entries overriding earlier
//! ones, its definitions shadow any user context that redefines a core term (a
//! user `@vocab`, a redeclared `observedAt`). A user context still contributes
//! its own new terms; it just cannot override the core's. [`resolved_context`]
//! implements that order once, as JSON text assembled from the entries the
//! request named.

use crate::{
    context_graph::reference_iri,
    context_spec::{ContextSpec, CoreContextPolicy},
    context_value::{ContextEntry, EntriesShape, without_core, written_entries},
    engine::context_url::is_absolute_iri,
    hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
};
use iri_rs::{IriBuf, IriRefBuf};
use std::{borrow::Cow, sync::Arc};

/// How the references a request named are read.
#[derive(Clone, Copy)]
pub enum ReferenceReading<'a> {
    /// As written, which is what identifies the context a spec selects.
    AsWritten,
    /// With every root-relative reference to one of the tenant's own stored
    /// `@context`s rewritten to its identifier, as the context is processed.
    Hosted(&'a dyn HostedContextRecognizer),
}

/// The `@context` a document is processed under: the contexts a request named,
/// in order and with their core references removed, then the core context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedContext {
    /// The value as JSON text, always an array.
    text: Arc<str>,
    /// The absolute references among its entries and the context each
    /// definition imports, in order: where the walk of its reference graph
    /// starts (`context_graph`).
    references: Vec<IriBuf>,
}

impl ResolvedContext {
    /// The value as JSON text: an array of the contexts processed in order.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text, shared: what identifies the processed context everywhere it
    /// is kept, since two specs resolving to the same text process the same
    /// value.
    #[must_use]
    pub fn key(&self) -> Arc<str> {
        Arc::clone(&self.text) // clone: Arc ref-count bump
    }

    /// The absolute references the value names directly or through a
    /// definition's `@import`.
    #[must_use]
    pub fn references(&self) -> &[IriBuf] {
        &self.references
    }
}

/// The `@context` a document is processed under for `spec`.
///
/// The one place the request named its context (clause 6.3.5: a `Link`
/// header or the body, never both) comes first, its core references removed,
/// and the core context last. Repeats are kept: JSON-LD processes an
/// `@context` array in order, so `[A, B, A]` leaves A's definitions in force
/// where `[A, B]` leaves B's, and clause 5.5.7 processes the context the call
/// provided.
#[must_use]
pub fn resolved_context(spec: &ContextSpec, reading: ReferenceReading<'_>) -> ResolvedContext {
    let core = spec.effective_core_url();
    let mut entries = match spec.core_policy() {
        CoreContextPolicy::Only => Vec::with_capacity(1),
        CoreContextPolicy::Append => without_core(spec.request_context().entries())
            .kept
            .into_iter()
            .map(|entry| read_reference(entry, reading))
            .collect(),
    };
    entries.push(ContextEntry::reference(core.as_str()));
    let references = entries
        .iter()
        .filter_map(|entry| match entry {
            ContextEntry::Reference(reference) => reference_iri(reference, None),
            ContextEntry::Definition(definition) => definition.import().and_then(|import| reference_iri(&import, None)),
            ContextEntry::Null | ContextEntry::Malformed(_) => None,
        })
        .collect();
    ResolvedContext {
        text: Arc::from(written_entries(&entries, EntriesShape::Array)),
        references,
    }
}

/// `entry`, a root-relative reference to one of the tenant's own stored
/// `@context`s rewritten to its identifier when `reading` asks for it.
///
/// A client that read a context from this broker may echo the path it read it
/// from as its `@context`. A request body has no base URL to resolve such a
/// reference against, so the processor could not dereference it; the
/// identifier is a URN, which the loader resolves from storage (clause 5.13.1
/// serves the document "on-demand"). An absolute reference is left as it is:
/// the loader recognises it when it is resolved. A relative reference that is
/// no IRI reference, or names no stored `@context` of the tenant, is left as
/// written for the processor to refuse.
fn read_reference<'a>(entry: ContextEntry<'a>, reading: ReferenceReading<'_>) -> ContextEntry<'a> {
    let ReferenceReading::Hosted(hosted) = reading else {
        return entry;
    };
    match entry {
        ContextEntry::Reference(reference) if !is_absolute_iri(&reference) => match IriRefBuf::new(reference.as_ref()) {
            Ok(relative) => match hosted.recognize(&relative) {
                HostedReference::Own(identifier) => ContextEntry::Reference(Cow::Owned(identifier.to_string())),
                HostedReference::Unrelated => ContextEntry::Reference(reference),
            },
            Err(_not_a_reference) => ContextEntry::Reference(reference),
        },
        other @ (ContextEntry::Null | ContextEntry::Reference(_) | ContextEntry::Definition(_) | ContextEntry::Malformed(_)) => other,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::{NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V18_URL},
        document_context::{ReferenceReading, resolved_context},
        hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::{IriBuf, IriRefBuf};
    use serde_json::json;

    fn body(value: &serde_json::Value) -> ContextSpec {
        ContextSpec::from_body(JsonLiteral::admit(&value.to_string()).expect("an admissible context"))
    }

    /// The resolved context of `spec`, read as written, as JSON.
    fn resolved(spec: &ContextSpec) -> serde_json::Value {
        serde_json::from_str(resolved_context(spec, ReferenceReading::AsWritten).text()).expect("the resolved context is JSON")
    }

    #[test]
    fn a_link_context_comes_first_and_the_core_context_last() {
        let spec = ContextSpec::from_link(IriBuf::new("https://example.com/a".to_owned()).expect("a valid IRI"));
        pretty_assertions::assert_eq!(resolved(&spec), json!(["https://example.com/a", NGSI_LD_CORE_CONTEXT_URL]));
    }

    /// JSON-LD 1.1 processes an `@context` array in order, so a repeated
    /// context is processed again and keeps the last word on the terms it
    /// defines (clause 5.5.7: the context the call provided).
    #[test]
    fn a_repeated_request_context_is_kept_in_place() {
        let spec = body(&json!(["https://example.com/a", "https://example.com/b", "https://example.com/a"]));
        pretty_assertions::assert_eq!(
            resolved(&spec),
            json!([
                "https://example.com/a",
                "https://example.com/b",
                "https://example.com/a",
                NGSI_LD_CORE_CONTEXT_URL
            ])
        );
    }

    #[test]
    fn a_request_naming_no_context_gets_the_core_context() {
        pretty_assertions::assert_eq!(resolved(&ContextSpec::none()), json!([NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_core_context_alias_the_request_named_is_replaced_by_the_canonical_one_last() {
        let spec = body(&json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_V16_URL.as_str()]));
        pretty_assertions::assert_eq!(resolved(&spec), json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_body_context_is_kept_and_the_core_context_still_comes_last() {
        let spec = body(&json!("https://example.com/compound.jsonld"));
        pretty_assertions::assert_eq!(resolved(&spec), json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    /// JSON-LD 1.1 section 4.1.10 and clause 5.5.7: a core context an inline
    /// definition imports is dropped like a referenced one, so the v1.9 core
    /// appended last is the only occurrence.
    #[test]
    fn a_core_context_an_inline_definition_imports_is_dropped_before_the_core_is_appended() {
        let spec = body(&json!({
            "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str(),
            "speed": "https://example.com/speed"
        }));
        pretty_assertions::assert_eq!(resolved(&spec), json!([{ "speed": "https://example.com/speed" }, NGSI_LD_CORE_CONTEXT_URL]));
    }

    /// A definition that imported the core context and set nothing else but
    /// `@version` or `@protected` leaves nothing behind, as the presented
    /// context leaves nothing behind for it.
    #[test]
    fn a_definition_holding_only_a_core_import_and_version_leaves_nothing_behind() {
        let spec = body(&json!({ "@import": NGSI_LD_CORE_CONTEXT_V18_URL.as_str(), "@version": 1.1 }));
        pretty_assertions::assert_eq!(resolved(&spec), json!([NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn a_core_only_spec_names_the_core_context_alone() {
        pretty_assertions::assert_eq!(resolved(&ContextSpec::core_only()), json!([NGSI_LD_CORE_CONTEXT_URL]));
    }

    /// The references a walk of the reference graph starts from: absolute
    /// references and imports, never the relative ones nothing resolves.
    #[test]
    fn the_absolute_references_and_imports_are_where_the_reference_walk_starts() {
        let spec = body(&json!(["https://example.com/a", "relative.jsonld", {"@import": "https://example.com/b"}]));
        let resolved = resolved_context(&spec, ReferenceReading::AsWritten);
        let references: Vec<&str> = resolved.references().iter().map(IriBuf::as_str).collect();
        pretty_assertions::assert_eq!(
            references,
            vec!["https://example.com/a", "https://example.com/b", NGSI_LD_CORE_CONTEXT_URL.as_str()]
        );
    }

    /// The stored identifier the recogniser below knows.
    const STORED: &str = "urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";

    /// Recognises the root-relative path of [`STORED`].
    #[derive(Debug)]
    struct StoredPath;

    impl HostedContextRecognizer for StoredPath {
        fn recognize(&self, reference: &IriRefBuf) -> HostedReference {
            if reference.as_str() == "/ngsi-ld/v1/jsonldContexts/stored" {
                HostedReference::Own(STORED.parse::<JsonLdContextId>().expect("a stored identifier"))
            } else {
                HostedReference::Unrelated
            }
        }
    }

    /// A request body has no base URL, so a root-relative reference to a
    /// stored context is processed under the identifier the tenant stores it
    /// under; any other relative reference is left for the processor.
    #[test]
    fn a_root_relative_reference_to_a_stored_context_is_processed_under_its_identifier() {
        let spec = body(&json!(["/ngsi-ld/v1/jsonldContexts/stored", "other.jsonld"]));
        let hosted = resolved_context(&spec, ReferenceReading::Hosted(&StoredPath));
        let read: serde_json::Value = serde_json::from_str(hosted.text()).expect("JSON");
        pretty_assertions::assert_eq!(read, json!([STORED, "other.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }
}
