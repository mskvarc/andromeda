//! Compacting one IRI back to the term a context spells it with.
//!
//! Compaction is defined over documents, not over names, so each of these
//! builds the smallest document that puts the IRI where compaction will act
//! on it and reads the term back out. An IRI that will not compact is
//! answered verbatim: clause 4.5.1 permits a rendering to carry a full IRI
//! when no term abbreviates it, so failing here would refuse a response that
//! is valid.

use crate::{
    context_spec::ContextSpec,
    engine::{
        context_url::is_full_iri,
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use serde_json::Value;

/// The options every compaction here uses: the caller wants a term, not a
/// document, so the `@context` is never kept.
const TERM_ONLY: CompactOptions = CompactOptions {
    strip_context: ContextStripping::Strip,
};

/// Compact an entity type IRI to the term a context spells it with.
///
/// Answers the IRI verbatim when it does not compact, or when it is not one
/// of the schemes an NGSI-LD type can use.
pub async fn compact_type_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    if !is_full_iri(iri) {
        return iri.to_string();
    }
    let document = serde_json::json!({ "@id": "urn:ngsi-ld:_:dummy", "@type": [iri] });

    match engine.compact(document, context, TERM_ONLY).await {
        Ok(compacted) => compacted.get("type").and_then(Value::as_str).map_or_else(|| iri.to_string(), String::from),
        Err(error) => {
            tracing::debug!(error = %error, iri, "type IRI did not compact; answering it verbatim");
            iri.to_string()
        }
    }
}

/// Compact an attribute IRI to the term a context spells it with.
///
/// Answers the IRI verbatim when it does not compact, or when it is not one
/// of the schemes an NGSI-LD attribute name can use.
pub async fn compact_attr_iri(engine: &JsonLdEngine, iri: &str, context: &ContextSpec) -> String {
    if !is_full_iri(iri) {
        return iri.to_string();
    }
    let document = serde_json::json!([{ "@id": "urn:ngsi-ld:_:dummy", iri: [{ "@value": "dummy" }] }]);

    match engine.compact(document, context, TERM_ONLY).await {
        // The compacted document holds the identifier and the one attribute;
        // whichever member is neither is the term being looked for.
        Ok(compacted) => compacted
            .as_object()
            .and_then(|object| object.keys().find(|key| *key != "id" && *key != "type" && !key.starts_with('@')))
            .map_or_else(|| iri.to_string(), String::clone),
        Err(error) => {
            tracing::debug!(error = %error, iri, "attribute IRI did not compact; answering it verbatim");
            iri.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compaction::{compact_attr_iri, compact_type_iri},
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::classes;

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    #[tokio::test]
    async fn a_name_that_is_not_an_iri_is_answered_verbatim() {
        let engine = offline_engine();
        let context = ContextSpec::none();

        pretty_assertions::assert_eq!(compact_type_iri(&engine, "Vehicle", &context).await, "Vehicle");
        pretty_assertions::assert_eq!(compact_attr_iri(&engine, "speed", &context).await, "speed");
    }

    #[tokio::test]
    async fn a_core_context_type_iri_compacts_back_to_its_term() {
        let compacted = compact_type_iri(&offline_engine(), classes::PROPERTY.as_str(), &ContextSpec::none()).await;
        pretty_assertions::assert_eq!(compacted, "Property");
    }

    #[tokio::test]
    async fn an_iri_no_context_abbreviates_is_answered_verbatim() {
        let engine = offline_engine();
        let context = ContextSpec::core_only();
        let iri = "https://example.com/vocab#unknownTerm";

        pretty_assertions::assert_eq!(compact_type_iri(&engine, iri, &context).await, iri);
        pretty_assertions::assert_eq!(compact_attr_iri(&engine, iri, &context).await, iri);
    }
}
