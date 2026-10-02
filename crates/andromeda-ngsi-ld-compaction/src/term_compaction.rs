//! A [`NameCompaction`], as a query-term compactor.
//!
//! `andromeda-ngsi-ld-query` states what it needs to spell an expanded filter
//! in a reader's terms ([`TermCompactor`]) and names no JSON-LD processor; the
//! compaction of one name against a processed context is this crate's, so the
//! binding lives here.

use crate::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::term_compactor::TermCompactor;
use std::future::{Future, ready};

/// The name a compacted spelling becomes, or the original when the spelling is
/// not one the clause 4.6.2 name grammar accepts.
///
/// Compaction answers verbatim whatever does not abbreviate, so the fallback
/// only opens if a context maps an IRI to a term an attribute cannot be keyed
/// by; the original is then the only name that still addresses it.
fn compacted_or_original(compacted: &str, original: &AttributeName) -> AttributeName {
    compacted.parse().unwrap_or_else(|_ungrammatical| original.clone()) // clone: the original stays in the filter being compacted
}

impl TermCompactor for NameCompaction {
    fn compact_term(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send {
        ready(compacted_or_original(&self.compact_attr_iri(name.as_str()), name))
    }

    fn compact_type(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send {
        ready(compacted_or_original(&self.compact_type_iri(name.as_str()), name))
    }
}

#[cfg(test)]
mod tests {
    use crate::name_compaction::NameCompaction;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
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

    async fn names(context: &ContextSpec) -> NameCompaction {
        let engine = offline_engine();
        let processed = engine.compactor(context).processed_context().await.expect("the context processes");
        NameCompaction::new(processed)
    }

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    #[tokio::test]
    async fn a_core_context_iri_compacts_back_to_its_term() {
        let compactor = names(&ContextSpec::none()).await;
        pretty_assertions::assert_eq!(compactor.compact_type(&name(classes::PROPERTY.as_str())).await.as_str(), "Property");
    }

    #[tokio::test]
    async fn an_iri_no_context_abbreviates_is_answered_as_given() {
        let compactor = names(&ContextSpec::core_only()).await;
        let iri = name("https://example.com/vocab#unknownTerm");
        pretty_assertions::assert_eq!(compactor.compact_term(&iri).await, iri);
        pretty_assertions::assert_eq!(compactor.compact_type(&iri).await, iri);
    }

    /// Clause 5.5.7: a `q` holding the default-context expansion of its terms
    /// is read back in the terms the core context spells them with.
    #[tokio::test]
    async fn a_default_context_filter_reads_back_in_its_terms() {
        let compactor = names(&ContextSpec::none()).await;
        let stored = parser::parse("\"https://uri.etsi.org/ngsi-ld/default-context/speed\">10").expect("parses");
        pretty_assertions::assert_eq!(serialize(&compact_q_expr(stored, &compactor).await), "speed>10");
    }
}
