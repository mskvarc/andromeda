//! The terms of a processed context whose container reshapes what is
//! compacted under them.
//!
//! Which terms do is a property of the processed context alone, read here once
//! for every reader of a document compacted under it. The containers the Core
//! `@context` gives a core term under its own name are the NGSI-LD shape of
//! that term and are left out.

use andromeda_ngsi_ld_representation::container_terms::{ContainerTerms, TermContainer};
use andromeda_ngsi_ld_vocabulary::expanded_to_compact;
use jsonld::{BlankIdBuf, ContainerKind, Context, IriBuf, Term, context::BindingRef, syntax::Keyword};

/// The container kinds that write the values of a term as the values of an
/// object keyed by what the container names.
const MAP_CONTAINERS: [ContainerKind; 4] = [ContainerKind::Index, ContainerKind::Id, ContainerKind::Type, ContainerKind::Graph];

/// Every term of `context` whose container reshapes what is compacted under
/// it.
#[must_use]
pub fn container_terms(context: &Context<IriBuf, BlankIdBuf>) -> ContainerTerms {
    let mut terms = ContainerTerms::none();
    for binding in context.definitions() {
        let (term, definition) = match binding {
            BindingRef::Normal(term, definition) => (term.as_str(), definition),
            BindingRef::Type(_) => continue,
        };
        let core_shape = match definition.value() {
            Some(Term::Keyword(Keyword::Nest)) => {
                terms.contain(term, TermContainer::Nest);
                continue;
            }
            Some(Term::Id(id)) => expanded_to_compact(id.as_str()) == Some(term),
            Some(Term::Keyword(_) | Term::Null) | None => false,
        };
        if core_shape {
            continue;
        }
        let container = definition.container;
        if MAP_CONTAINERS.into_iter().any(|kind| container.contains(kind)) {
            terms.contain(term, TermContainer::Map);
        } else if container.contains(ContainerKind::Set) && !container.contains(ContainerKind::List) {
            terms.contain(term, TermContainer::Set);
        }
    }
    terms
}

#[cfg(test)]
mod tests {
    use crate::{container_terms::container_terms, context_spec::ContextSpec, engine::processor::JsonLdEngine, test_documents::literal};
    use andromeda_ngsi_ld_representation::container_terms::{ContainerTerms, TermContainer};
    use serde_json::{Value, json};

    /// The container terms of `context`, processed with the Core `@context`.
    async fn containers_of(context: &Value) -> ContainerTerms {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
        let processed = engine
            .expansion_context(&ContextSpec::from_body(literal(context)))
            .await
            .expect("the context processes");
        container_terms(processed.active())
    }

    #[tokio::test]
    async fn the_set_map_and_nest_terms_of_a_context_are_recorded() {
        let terms = containers_of(&json!({
            "tps": {"@id": "@type", "@container": "@set"},
            "speed": {"@id": "https://example.org/speed", "@container": "@index"},
            "n": "@nest",
            "seq": {"@id": "https://example.org/seq", "@container": "@list"}
        }))
        .await;
        pretty_assertions::assert_eq!(terms.container_of("tps"), Some(TermContainer::Set));
        pretty_assertions::assert_eq!(terms.container_of("speed"), Some(TermContainer::Map));
        pretty_assertions::assert_eq!(terms.container_of("n"), Some(TermContainer::Nest));
        pretty_assertions::assert_eq!(terms.container_of("seq"), None);
    }

    /// The Core `@context` writes `languageMap` as an `@language` map, which
    /// is the shape clause 4.5.18 gives it.
    #[tokio::test]
    async fn the_core_context_reshapes_nothing() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
        let processed = engine.expansion_context(&ContextSpec::core_only()).await.expect("the core context processes");
        assert!(container_terms(processed.active()).is_empty());
    }
}
