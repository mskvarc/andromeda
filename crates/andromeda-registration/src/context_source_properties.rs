//! JSON-LD normalization of Context Source properties used by `csf`.

use crate::{error::RegistrationError, registration_validation_reason::RegistrationValidationReason};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use indexmap::IndexMap;
use std::str::FromStr;

/// Expand the names of the Context Source properties a registration body
/// carries.
///
/// A `csf` expression is expanded against the request context before evaluation,
/// so its target registration properties must use the same canonical IRI keys and
/// stored key. Context Source Property values retain their registration wire
/// shape because CSR query pushdown unwraps that shape directly.
///
/// # Errors
///
/// Returns [`RegistrationError::JsonLd`] when the context cannot be processed,
/// and [`RegistrationValidationReason::ExpandedName`] when a term expands to a
/// name NGSI-LD cannot carry.
pub(super) async fn expand_context_source_properties(
    engine: &JsonLdEngine,
    properties: IndexMap<AttributeName, JsonLiteral>,
    context: &ContextSpec,
) -> Result<IndexMap<AttributeName, JsonLiteral>, RegistrationError> {
    if properties.is_empty() {
        return Ok(properties);
    }
    let prepared_context = engine.prepare_term_context(context);
    let mut expanded = IndexMap::with_capacity(properties.len());
    for (name, value) in properties {
        let iri = engine.expand_term(name.as_str(), &prepared_context).await?;
        let name = AttributeName::from_str(iri.as_str()).map_err(|source| RegistrationValidationReason::ExpandedName { source })?;
        expanded.insert(name, value);
    }
    Ok(expanded)
}

/// Compact the names of the stored Context Source properties for a
/// registration response.
///
/// A term the context compacts a name to is used where it is a name NGSI-LD can
/// carry; one that is not leaves the name in the expanded form it is stored in.
pub(super) fn compact_context_source_properties(
    names: &NameCompaction,
    properties: IndexMap<AttributeName, JsonLiteral>,
) -> IndexMap<AttributeName, JsonLiteral> {
    properties
        .into_iter()
        .map(|(name, value)| {
            let compacted = AttributeName::from_str(&names.compact_attr_iri(name.as_str())).unwrap_or(name);
            (compacted, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::context_source_properties::{compact_context_source_properties, expand_context_source_properties};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use indexmap::IndexMap;

    fn properties(members: &[(&str, &str)]) -> IndexMap<AttributeName, JsonLiteral> {
        members
            .iter()
            .map(|(name, value)| (name.parse().expect("a valid name"), JsonLiteral::admit(value).expect("a JSON literal")))
            .collect()
    }

    fn names(map: &IndexMap<AttributeName, JsonLiteral>) -> Vec<&str> {
        map.keys().map(AttributeName::as_str).collect()
    }

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// Clause 5.5.7: a property's term expands under the request context to the
    /// name it is stored and matched under, and the values are kept as written.
    #[tokio::test]
    async fn property_terms_expand_under_the_request_context_and_keep_their_values() {
        let context = ContextSpec::from_body(JsonLiteral::admit(r#"{"tier": "https://example.org/tier"}"#).expect("an admissible literal"));

        let expanded = expand_context_source_properties(&engine(), properties(&[("tier", r#"{"type": "Property", "value": 3}"#)]), &context)
            .await
            .expect("the terms expand");

        pretty_assertions::assert_eq!(names(&expanded), vec!["https://example.org/tier"]);
        pretty_assertions::assert_eq!(expanded.values().next().map(JsonLiteral::text), Some(r#"{"type":"Property","value":3}"#));
    }

    /// Clause 5.2.9: the names compact under the response's context, and a name
    /// no term names stays expanded.
    #[tokio::test]
    async fn stored_property_names_compact_under_the_response_context() {
        let engine = engine();
        let compaction = NameCompaction::new(
            engine
                .compactor(&ContextSpec::none())
                .processed_context()
                .await
                .expect("the core context processes"),
        );

        let compacted = compact_context_source_properties(
            &compaction,
            properties(&[
                ("https://uri.etsi.org/ngsi-ld/default-context/speed", "1"),
                ("https://example.org/unknown", "2"),
            ]),
        );

        pretty_assertions::assert_eq!(names(&compacted), vec!["speed", "https://example.org/unknown"]);
    }
}
