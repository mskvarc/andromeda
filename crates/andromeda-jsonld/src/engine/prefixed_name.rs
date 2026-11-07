//! The clause 4.6.2 grammar of a name an active context reads as a compact
//! IRI.
//!
//! Clause 4.6.2 admits a prefixed name only as `prefix:name`, the part after
//! the colon following the unprefixed grammar. Whether a written name is
//! prefixed is the context's decision: JSON-LD 1.1 IRI expansion (step 6.4)
//! reads `ex:bad-name` as a compact IRI when `ex` is a term usable as a prefix,
//! and keeps `urn:x-y` as the absolute IRI it is when `urn` is no term. The
//! check therefore runs against the active context, before expansion leaves
//! only the IRI.

use andromeda_ngsi_ld_lexicon::name::unprefixed::is_unprefixed_name;
use iri_rs::IriBuf;
use jsonld::{Context as ActiveContext, ProcessingMode, syntax::CompactIri};
use rdfx::BlankIdBuf;
use thiserror::Error;

/// A name read as a compact IRI whose part after the colon is no clause 4.6.2
/// name.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("the part after the prefix must start with a letter and hold only letters, digits and '_'")]
pub struct PrefixedNameRefusal;

/// Check `name` against the clause 4.6.2 `prefix:name` form when `active`
/// reads it as a compact IRI; any other name passes, its own grammar being
/// checked where it is parsed.
///
/// # Errors
///
/// Returns [`PrefixedNameRefusal`] for a compact IRI whose part after the colon
/// is not an unprefixed name.
pub fn check_prefixed_name(active: &ActiveContext<IriBuf, BlankIdBuf>, name: &str) -> Result<(), PrefixedNameRefusal> {
    let Ok(compact_iri) = CompactIri::new(name) else {
        return Ok(());
    };
    // The prefix flag is a JSON-LD 1.1 addition: in 1.0 every term definition
    // with an IRI mapping expands a compact IRI, as the processor does.
    let prefixed = active
        .get(compact_iri.prefix())
        .is_some_and(|definition| (definition.prefix() || active.processing_mode() == ProcessingMode::JsonLd1_0) && definition.value().is_some());
    if prefixed && !is_unprefixed_name(compact_iri.suffix()) {
        return Err(PrefixedNameRefusal);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        engine::{
            expansion_context::ExpansionContext,
            prefixed_name::{PrefixedNameRefusal, check_prefixed_name},
            processor::JsonLdEngine,
        },
        test_documents::literal,
    };
    use serde_json::json;
    use std::sync::Arc;

    /// The context `{"ex": "http://example.org/"}` processed with the core
    /// context.
    async fn example_prefix_context() -> Arc<ExpansionContext> {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let spec = ContextSpec::from_body(literal(&json!({ "ex": "http://example.org/" })));
        engine.expansion_context(&spec).await.expect("the context processes")
    }

    #[tokio::test]
    async fn a_prefixed_name_whose_part_after_the_colon_breaks_the_grammar_is_refused() {
        let context = example_prefix_context().await;
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "ex:bad-name"), Err(PrefixedNameRefusal));
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "ex:42x"), Err(PrefixedNameRefusal));
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "ex:a:b"), Err(PrefixedNameRefusal));
    }

    #[tokio::test]
    async fn a_prefixed_name_following_the_grammar_passes() {
        let context = example_prefix_context().await;
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "ex:good_name42"), Ok(()));
    }

    /// `urn` is no term of the context, so `urn:x-y` is the absolute IRI it is
    /// written as; a suffix starting `//` is never a compact IRI (JSON-LD 1.1
    /// IRI expansion step 6.2).
    #[tokio::test]
    async fn an_absolute_iri_and_an_unprefixed_name_are_not_checked_as_prefixed_names() {
        let context = example_prefix_context().await;
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "urn:x-y"), Ok(()));
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "ex://host/bad-name"), Ok(()));
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "https://example.org/bad-name"), Ok(()));
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "speed"), Ok(()));
    }

    /// The core context defines `ngsi-ld` as a prefix, so its own prefix is
    /// held to the same grammar after the colon.
    #[tokio::test]
    async fn a_prefix_the_core_context_defines_is_checked_as_well() {
        let context = example_prefix_context().await;
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "ngsi-ld:bad-name"), Err(PrefixedNameRefusal));
        pretty_assertions::assert_eq!(check_prefixed_name(context.active(), "ngsi-ld:goodName"), Ok(()));
    }
}
