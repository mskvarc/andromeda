//! Counting what a stored entity holds against the entity value limit.
//!
//! The written document of a request is counted on its own (see
//! [`crate::document_census`]), but a merge, an append or a Temporal Evolution
//! fold adds what it writes to what the entity already holds, so writes each
//! within the limit can still grow one entity past it. Every write that stores
//! a modified entity is therefore also held to the limit on the state it
//! stores, counted with the weights the document census gives the same
//! members, so a document and the entity stored from it count alike.
//!
//! A limit lowered while the broker holds entities above it leaves them
//! writable: a write is refused only when it leaves the entity above the
//! limit and holding more values than before, so a write that shrinks such an
//! entity, or restates it at its size, still lands.

use andromeda_ngsi_ld_entity::{
    attributes::Attributes,
    entity_state::EntityState,
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    instance::Instance,
};
use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueCount, JsonValueLimit};
use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;

/// Admit `current` stored in place of `previous` (`None` when the write
/// creates the entity) under `limit`.
///
/// # Errors
///
/// Returns [`PayloadBudgetExceeded::EntityJsonValues`] when `current` holds
/// more values than `limit` admits and more than `previous` held.
pub fn admit_stored_entity(limit: JsonValueLimit, previous: Option<&EntityState>, current: &EntityState) -> Result<(), PayloadBudgetExceeded> {
    let stored = stored_value_count(current);
    if stored.get() <= limit.get() || previous.is_some_and(|previous| stored_value_count(previous) >= stored) {
        return Ok(());
    }
    Err(PayloadBudgetExceeded::EntityJsonValues { limit })
}

/// The JSON values `state` holds, geometry positions and JSON literals aside.
#[must_use]
pub fn stored_value_count(state: &EntityState) -> JsonValueCount {
    let scope = state.scope().map_or(0, |scopes| scopes.as_slice().len());
    let head = 2 + state.types().as_slice().len() + scope + usize::from(state.expires_at().is_some());
    JsonValueCount::new(head.saturating_add(attributes(state.attributes())))
}

/// The values of every attribute of an entity or an instance.
fn attributes(attributes: &Attributes) -> usize {
    attributes
        .iter()
        .map(|(_, instances)| {
            instances
                .iter()
                .map(|(dataset_id, held)| instance(dataset_id.is_some(), held))
                .fold(1, usize::saturating_add)
        })
        .fold(0, usize::saturating_add)
}

/// The values of one instance: the instance, its family and every member it
/// holds, its value and its sub-attributes.
fn instance(dataset_id: bool, instance: &Instance) -> usize {
    let value = instance.value();
    let members = [
        dataset_id,
        instance.observed_at().is_some(),
        instance.expires_at().is_some(),
        value.unit_code().is_some(),
        value.value_type().is_some(),
    ];
    let head = 2 + members.into_iter().filter(|held| *held).count() + value.object_types().len();
    head.saturating_add(family_value(value)).saturating_add(attributes(instance.sub_attributes()))
}

/// The values one family value holds.
fn family_value(value: &FamilyValue) -> usize {
    match value {
        FamilyValue::Property { values, .. } => expanded_values(values),
        FamilyValue::ListProperty { items, .. } => expanded_values(items).saturating_add(1),
        FamilyValue::GeoProperty { .. } => 1,
        FamilyValue::Relationship { objects, .. } => objects.as_slice().len(),
        FamilyValue::LanguageProperty { entries, .. } => 1 + entries.entries().len(),
        FamilyValue::VocabProperty { vocab, .. } => vocab.as_slice().len(),
        FamilyValue::JsonProperty { json, .. } => json.as_slice().len(),
        FamilyValue::ListRelationship { objects, .. } => 1 + objects.len(),
    }
}

/// The values of `values` and every value below them.
fn expanded_values(values: &[ExpandedValue]) -> usize {
    values.iter().map(expanded_value).fold(0, usize::saturating_add)
}

/// One value and every value below it.
fn expanded_value(value: &ExpandedValue) -> usize {
    let below = match value {
        ExpandedValue::Literal(_) | ExpandedValue::Json(_) => 0,
        ExpandedValue::List(items) => expanded_values(items),
        ExpandedValue::Node(node) => node.members().values().map(|values| expanded_values(values)).fold(0, usize::saturating_add),
    };
    below.saturating_add(1)
}

#[cfg(test)]
mod tests {
    use crate::{
        document_census::admit_entity_document,
        stored_census::{admit_stored_entity, stored_value_count},
    };
    use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_representation::{
        attribute_nesting::AttributeNestingLimit,
        payload_budget::{EntityPayloadBudget, PayloadBudgetExceeded},
    };
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    fn limit(values: usize) -> JsonValueLimit {
        JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive value limit"))
    }

    fn budget(values: usize) -> EntityPayloadBudget {
        EntityPayloadBudget {
            json_values: limit(values),
            geometry_positions: GeometryPositionLimit::new(NonZeroUsize::new(1_000).expect("a positive position limit")),
            attribute_nesting: AttributeNestingLimit::DECODABLE_CEILING,
        }
    }

    fn document(expanded: &Value) -> EntityDocument {
        EntityDocument::from_slice(&serde_json::to_vec(expanded).expect("JSON")).expect("a written document")
    }

    fn state(expanded: &Value) -> EntityState {
        EntityState::try_from(document(expanded)).expect("a storable entity")
    }

    /// A Probe whose `n` Property holds `count` numbers.
    fn probe(count: i64) -> Value {
        let items: Vec<Value> = (0..count).map(|item| json!({"@value": item})).collect();
        json!([{
            "@id": "urn:ngsi-ld:Probe:1",
            "@type": ["https://example.org/Probe"],
            "https://example.org/n": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": items}]
        }])
    }

    /// One payload of every family, a dataset, observedAt, a unitCode, a
    /// structured value, a list, two relationship targets and a sub-Property.
    fn every_family() -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle", "https://example.org/Car"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/a"}, {"@value": "/b"}],
            "https://example.org/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "KMH"}],
                "https://uri.etsi.org/ngsi-ld/observedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2026-01-01T00:00:00Z"}],
                "https://example.org/accuracy": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 2}]}]
            }, {
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 3}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:gps"}]
            }],
            "https://example.org/address": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"https://example.org/street": [{"@value": "Main"}], "https://example.org/numbers": [{"@list": [{"@value": 1}, {"@value": 2}]}]}]
            }],
            "https://example.org/owner": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Relationship"],
                "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Person:1"}, {"@id": "urn:ngsi-ld:Person:2"}]
            }],
            "https://example.org/name": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"],
                "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [{"@value": "car", "@language": "en"}, {"@value": "auto", "@language": "sl"}]
            }],
            "https://example.org/stops": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "a"}, {"@value": "b"}]}]
            }],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1.0}, {"@value": 2.0}]}]
                }]
            }],
            "https://example.org/settings": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/JsonProperty"],
                "https://uri.etsi.org/ngsi-ld/hasJSON": [{"@type": "@json", "@value": {"a": [1, 2, 3]}}]
            }]
        }])
    }

    /// The stored census gives each member the weight the document census
    /// gives it, so the entity stored from a document counts as the document
    /// did: the document is admitted at exactly its stored count and refused
    /// one below it.
    #[test]
    fn a_stored_entity_counts_as_the_document_it_was_stored_from() {
        let payload = every_family();
        let counted = stored_value_count(&state(&payload)).get();
        admit_entity_document(budget(counted), &document(&payload)).expect("the document fits its stored count");
        assert!(
            matches!(
                admit_entity_document(budget(counted - 1), &document(&payload)),
                Err(PayloadBudgetExceeded::EntityJsonValues { .. })
            ),
            "the document holds exactly {counted} values"
        );
    }

    /// The head (node, id, one type), the attribute, the instance, its family
    /// and 20 numbers.
    #[test]
    fn every_value_of_a_property_is_counted() {
        pretty_assertions::assert_eq!(stored_value_count(&state(&probe(20))).get(), 3 + 1 + 2 + 20);
    }

    #[test]
    fn a_created_entity_within_the_limit_is_admitted_and_one_past_it_is_refused() {
        let created = state(&probe(20));
        admit_stored_entity(limit(26), None, &created).expect("26 values are within 26");
        pretty_assertions::assert_eq!(
            admit_stored_entity(limit(25), None, &created),
            Err(PayloadBudgetExceeded::EntityJsonValues { limit: limit(25) })
        );
    }

    /// Writes each within the limit cannot grow an entity past it.
    #[test]
    fn a_write_growing_an_entity_past_the_limit_is_refused() {
        let previous = state(&probe(10));
        let grown = state(&probe(30));
        pretty_assertions::assert_eq!(
            admit_stored_entity(limit(30), Some(&previous), &grown),
            Err(PayloadBudgetExceeded::EntityJsonValues { limit: limit(30) })
        );
        admit_stored_entity(limit(36), Some(&previous), &grown).expect("36 values are within 36");
    }

    /// An entity above a limit lowered after it was stored can still be
    /// shrunk or restated at its size, but not grown.
    #[test]
    fn an_entity_above_a_lowered_limit_may_shrink_but_not_grow() {
        let held = state(&probe(30));
        admit_stored_entity(limit(10), Some(&held), &state(&probe(20))).expect("shrinking is admitted");
        admit_stored_entity(limit(10), Some(&held), &state(&probe(30))).expect("the same size is admitted");
        pretty_assertions::assert_eq!(
            admit_stored_entity(limit(10), Some(&held), &state(&probe(31))),
            Err(PayloadBudgetExceeded::EntityJsonValues { limit: limit(10) })
        );
    }
}
