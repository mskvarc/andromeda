//! A [`NameCompactor`], as a query-term compactor.
//!
//! The mirror of [`crate::term_expansion`]: `andromeda-ngsi-ld-query` states
//! what it needs to spell a stored filter in a reader's terms and names
//! nothing here; binding the compactor one rendering already holds to that
//! trait happens on this side of the boundary.

use crate::compaction::NameCompactor;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::term_compactor::TermCompactor;
use std::future::Future;

/// The name a compacted spelling becomes, or the original when the spelling
/// is not one the clause 4.6.2 name grammar accepts.
///
/// Compaction answers verbatim whatever does not abbreviate, so the fallback
/// only opens if a context maps an IRI to a term an attribute cannot be keyed
/// by; the original is then the only name that still addresses the stored
/// attribute.
fn compacted_or_original(compacted: &str, original: &AttributeName) -> AttributeName {
    compacted.parse().unwrap_or_else(|_ungrammatical| original.clone()) // clone: the original stays in the filter being rendered
}

// Spelled with the trait's own `impl Future` return rather than as `async fn`
// bodies, for the same rustdoc reason `ContextualExpander` gives
// (rust-lang/rust#100013).
impl TermCompactor for NameCompactor<'_> {
    fn compact_term(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send {
        let names: &NameCompactor<'_> = self;
        async move {
            let compacted = names.compact_attr_iri(name.as_str()).await;
            compacted_or_original(&compacted, name)
        }
    }

    fn compact_type(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send {
        let names: &NameCompactor<'_> = self;
        async move {
            let compacted = names.compact_type_iri(name.as_str()).await;
            compacted_or_original(&compacted, name)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{compaction::NameCompactor, context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        filter::{compaction::compact_q_expr, parser, serialize::serialize},
        term_compactor::TermCompactor,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::classes;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    #[tokio::test]
    async fn a_core_context_iri_compacts_back_to_its_term() {
        let engine = offline_engine();
        let context = ContextSpec::none();
        let compactor = NameCompactor::new(&engine, &context);

        let compacted = compactor.compact_type(&name(classes::PROPERTY.as_str())).await;

        pretty_assertions::assert_eq!(compacted.as_str(), "Property");
    }

    #[tokio::test]
    async fn an_iri_no_context_abbreviates_is_answered_as_given() {
        let engine = offline_engine();
        let context = ContextSpec::core_only();
        let compactor = NameCompactor::new(&engine, &context);
        let iri = name("https://example.com/vocab#unknownTerm");

        pretty_assertions::assert_eq!(compactor.compact_term(&iri).await, iri);
        pretty_assertions::assert_eq!(compactor.compact_type(&iri).await, iri);
    }

    /// Clause 5.5.7: a `q` stored with the default-context expansion of its
    /// terms is read back in the terms the core context spells them with.
    #[tokio::test]
    async fn a_default_context_filter_reads_back_in_its_terms() {
        let engine = offline_engine();
        let context = ContextSpec::none();
        let compactor = NameCompactor::new(&engine, &context);
        let stored = parser::parse("\"https://uri.etsi.org/ngsi-ld/default-context/speed\">10").expect("parses");

        let compacted = compact_q_expr(stored, &compactor).await;

        pretty_assertions::assert_eq!(serialize(&compacted), "speed>10");
    }
}
