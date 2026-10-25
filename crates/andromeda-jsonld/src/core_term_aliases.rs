//! The terms a processed context defines for the core NGSI-LD terms.
//!
//! A request written under a user `@context` (clause 5.5.7) and a document
//! compacted under a request's context (JSON-LD 1.1 API section 6.2.2) may name
//! a core term by a term of the context: an alias of `@type`, a shorter term
//! for `ngsi-ld:hasValue`. Which terms do is a property of the processed
//! context alone, read here once for every reader of a compact document.

use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
use andromeda_ngsi_ld_vocabulary::{expanded_to_compact, keyword};
use jsonld::{BlankIdBuf, Context, IriBuf, Term, context::BindingRef, syntax::Keyword};

/// Every term `context` defines for a core term, mapped to that core term,
/// beside the clause 4.4 keyword spellings every context reads.
#[must_use]
pub fn core_term_aliases(context: &Context<IriBuf, BlankIdBuf>) -> CoreTermAliases {
    let mut aliases = CoreTermAliases::keyword_spellings().clone(); // clone: the table of this context extends the shared one
    for binding in context.definitions() {
        let (term, definition) = match binding {
            BindingRef::Normal(term, definition) => (term.as_str(), definition),
            BindingRef::Type(_) => continue,
        };
        let core = match definition.value() {
            Some(Term::Keyword(Keyword::Id)) => Some(keyword::ID_ALIAS),
            Some(Term::Keyword(Keyword::Type)) => Some(keyword::TYPE_ALIAS),
            Some(Term::Id(id)) => expanded_to_compact(id.as_str()),
            Some(Term::Keyword(_) | Term::Null) | None => None,
        };
        if let Some(core) = core {
            aliases.alias(term, core);
        }
    }
    aliases
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{context_spec::ContextSpec, core_term_aliases::core_term_aliases, engine::processor::JsonLdEngine, test_documents::literal};
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
    use andromeda_ngsi_ld_vocabulary::expanded_to_compact;
    use serde_json::{Value, json};
    use std::borrow::Cow;

    /// The aliases of `context`, processed with the Core `@context` appended.
    async fn aliases_of(context: &Value) -> CoreTermAliases {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
        let processed = engine
            .expansion_context(&ContextSpec::from_body(literal(context)))
            .await
            .expect("the context processes");
        core_term_aliases(processed.active())
    }

    /// Every value carrier names its core term, which an alias is read as.
    #[test]
    fn every_value_carrier_resolves_to_its_core_term() {
        for (iri, core) in [
            ("https://uri.etsi.org/ngsi-ld/hasValue", "value"),
            ("https://uri.etsi.org/ngsi-ld/hasObject", "object"),
            ("https://uri.etsi.org/ngsi-ld/hasJSON", "json"),
            ("https://uri.etsi.org/ngsi-ld/hasVocab", "vocab"),
            ("https://uri.etsi.org/ngsi-ld/hasLanguageMap", "languageMap"),
            ("https://uri.etsi.org/ngsi-ld/hasValueList", "valueList"),
            ("https://uri.etsi.org/ngsi-ld/hasObjectList", "objectList"),
        ] {
            pretty_assertions::assert_eq!(expanded_to_compact(iri), Some(core), "{iri}");
        }
    }

    #[tokio::test]
    async fn a_context_term_for_type_aliases_type() {
        let aliases = aliases_of(&json!({"kind": "@type", "ident": "@id"})).await;
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("kind")), "type");
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("ident")), "id");
    }

    #[tokio::test]
    async fn a_context_term_for_has_value_aliases_value() {
        let aliases = aliases_of(&json!({"v": "https://uri.etsi.org/ngsi-ld/hasValue"})).await;
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("v")), "value");
    }

    /// A context aliasing no core term still reads the clause 4.4 keyword
    /// spellings.
    #[tokio::test]
    async fn a_context_aliasing_nothing_reads_the_keyword_spellings() {
        let aliases = aliases_of(&json!({"speed": "https://example.org/speed"})).await;
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("@type")), "type");
        pretty_assertions::assert_eq!(aliases.core_name(Cow::Borrowed("speed")), "speed");
    }

    /// The Core `@context` defines each core term under its own name, so it
    /// aliases nothing beyond the keyword spellings.
    #[tokio::test]
    async fn the_core_context_aliases_only_the_keyword_spellings() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
        let processed = engine.expansion_context(&ContextSpec::core_only()).await.expect("the core context processes");
        pretty_assertions::assert_eq!(&core_term_aliases(processed.active()), CoreTermAliases::keyword_spellings());
    }
}
