//! The aggregated representation of an entity under the request context
//! (clause 4.5.19).
//!
//! The aggregation keys (`avg`, `min`, ...) are not IRIs, so the
//! representation is not a document compaction could read; it is built in
//! compact form directly, each attribute and type named by the term the request
//! context gives it (clause 5.7.3). A name the resolved context does not
//! declare stays a full IRI rather than being truncated to a bare fragment the
//! client could not expand back.

use crate::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    entity_type::EntityTypeValue,
    temporal::{TemporalAggregationRow, aggregated_entity::AggregatedTemporalEntity},
};
use std::str::FromStr;

/// The aggregated entity `rows` make of `id`, whose `expanded_types`, and every
/// attribute the rows name, are compacted by `names`.
///
/// `grouping_depth` is how many grouping terms nest the triples of each
/// method, none for the standard format.
#[must_use]
pub fn aggregated_entity(
    id: Identifier,
    expanded_types: &[EntityTypeName],
    rows: &[TemporalAggregationRow],
    methods: &[AggrMethod],
    grouping_depth: usize,
    names: &NameCompaction,
) -> AggregatedTemporalEntity {
    let mut compacted: Vec<EntityTypeName> = expanded_types
        .iter()
        .map(|expanded| {
            let term = names.compact_type_iri(expanded.as_str());
            // A term that is no type name leaves the type expanded.
            EntityTypeName::from_str(&term).unwrap_or_else(|_| expanded.clone()) // clone: the aggregated entity owns the type name
        })
        .collect();
    let type_ = match compacted.len() {
        1 => compacted.pop().map_or(EntityTypeValue::Multiple(Vec::new()), EntityTypeValue::Single),
        _ => EntityTypeValue::Multiple(compacted),
    };
    AggregatedTemporalEntity::from_rows(id, type_, rows, methods, grouping_depth, |expanded| {
        let term = names.compact_attr_iri(expanded.as_str());
        // A term that is no attribute name leaves the attribute expanded.
        AttributeName::from_str(&term).unwrap_or_else(|_| expanded.clone()) // clone: the aggregated entity owns the attribute name
    })
}

#[cfg(test)]
mod tests {
    use crate::{aggregated_entity::aggregated_entity, name_compaction::NameCompaction};
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType};
    use andromeda_ngsi_ld_model::temporal::TemporalAggregationRow;

    /// A row of one attribute, every aggregate absent.
    fn row(attribute: &str) -> TemporalAggregationRow {
        TemporalAggregationRow {
            attribute_name: attribute.parse().expect("an attribute name"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            bucket_start: None,
            bucket_end: None,
            avg: None,
            min: None,
            max: None,
            sum: None,
            sumsq: None,
            stddev: None,
            total_count: Some(1),
            distinct_count: None,
            group_keys: Vec::new(),
        }
    }

    /// Attribute and type names are those the request context gives them, a
    /// name it does not declare staying a full IRI.
    #[tokio::test]
    async fn names_are_compacted_against_the_request_context() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
        let core = ContextSpec::core_only();
        let compactor = engine.compactor(&core);
        let names = NameCompaction::new(compactor.processed_context().await.expect("the core context processes"));
        let aggregated = aggregated_entity(
            "urn:ngsi-ld:Vehicle:1".parse().expect("an identifier"),
            &["https://uri.etsi.org/ngsi-ld/default-context/Vehicle".parse().expect("a type")],
            &[row("https://uri.etsi.org/ngsi-ld/location"), row("https://example.com/unknown")],
            &[AggrMethod::TotalCount],
            0,
            &names,
        );
        let keys: Vec<&str> = aggregated.attrs.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(keys, vec!["location", "https://example.com/unknown"]);
    }
}
