//! What a member key expands to under an active context.
//!
//! JSON-LD 1.1 expansion drops a key that expands to neither an IRI nor a
//! keyword (Expansion Algorithm step 13.3), so the member it names is lost
//! rather than refused. A reader that has to tell such a key apart reads its
//! expansion here, through the IRI expansion the processor makes for a key.

use iri_rs::IriBuf;
use jsonld::{
    Context as ActiveContext,
    Id,
    Term,
    ValidId,
    context_processing::algorithm::{Action, expand_property_key},
};
use rdfx::{BlankIdBuf, vocabulary::no_vocabulary_mut};

/// What a member key expands to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum KeyExpansion {
    /// The key names the member of this IRI.
    Iri(IriBuf),
    /// The key is a JSON-LD keyword, or a term mapped to one.
    Keyword,
    /// The key expands to no IRI: to null, to a blank node identifier or to
    /// text that is no IRI, so expansion drops the member it names.
    NoIri,
}

/// Expand `key` as a member key of `active`, vocabulary-relative and not
/// document-relative.
#[must_use]
pub fn expand_key(active: &ActiveContext<IriBuf, BlankIdBuf>, key: &str) -> KeyExpansion {
    // `Keep` never rejects and never drops, so neither outcome is reached.
    match expand_property_key(no_vocabulary_mut(), active, key, Action::Keep).ok().flatten().as_deref() {
        Some(Term::Id(Id::Valid(ValidId::Iri(iri)))) => KeyExpansion::Iri(iri.clone()), // clone: the processor's memoized term stays shared
        Some(Term::Keyword(_)) => KeyExpansion::Keyword,
        Some(Term::Null | Term::Id(Id::Valid(ValidId::Blank(_)) | Id::Invalid(_))) | None => KeyExpansion::NoIri,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        engine::{
            key_expansion::{KeyExpansion, expand_key},
            processor::JsonLdEngine,
        },
        test_documents::literal,
    };
    use serde_json::json;

    #[tokio::test]
    async fn a_key_expands_to_its_iri_a_keyword_or_no_iri() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let spec = ContextSpec::from_body(literal(&json!({ "ex": "https://example.org/" })));
        let context = engine.expansion_context(&spec).await.expect("the context processes");
        let iri = |text: &str| KeyExpansion::Iri(text.parse().expect("an IRI"));
        pretty_assertions::assert_eq!(expand_key(context.active(), "ex:a"), iri("https://example.org/a"));
        pretty_assertions::assert_eq!(expand_key(context.active(), "speed"), iri("https://uri.etsi.org/ngsi-ld/default-context/speed"));
        pretty_assertions::assert_eq!(expand_key(context.active(), "id"), KeyExpansion::Keyword);
        for key in ["ex:a{b", "ex:{", "ex:%", "ex:\"", "urn:x:{", "https://example.org/{"] {
            pretty_assertions::assert_eq!(expand_key(context.active(), key), KeyExpansion::NoIri, "{key}");
        }
    }
}
