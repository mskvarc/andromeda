//! Everything one entity write must land together.
//!
//! Clauses 5.6.1 through 5.6.6 make one entity the atomic unit, while clauses
//! 5.6.7 through 5.6.10 permit per-entity partial success. A backend may fuse
//! several independent writes without changing that contract.

use crate::{
    batch_side_effect_job::BatchSideEffectJob,
    entity_mutation::EntityMutation,
    history_record::HistoryRecord,
    precondition::Precondition,
    subscription_match::SubscriptionMatch,
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::entity_member::extract_types;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// One entity mutation and every consequence that must share its commit.
#[derive(Debug, TypedBuilder)]
pub struct AtomicWrite {
    /// Entity this write concerns.
    pub entity_id: Arc<Uri>,
    /// Mutation of the current entity row.
    pub entity: EntityMutation,
    /// Temporal Evolution records produced by the mutation.
    #[builder(default = Default::default())]
    pub history: Vec<HistoryRecord>,
    /// Subscriptions the mutation matched, each owed a notification event.
    #[builder(default = Default::default())]
    pub subscription_matches: Vec<SubscriptionMatch>,
    /// Durable job used by the opt-in asynchronous batch path.
    #[builder(default = Default::default())]
    pub side_effects: Option<BatchSideEffectJob>,
    /// Assumption the mutation makes about current entity state.
    #[builder(default = Precondition::None)]
    pub precondition: Precondition,
}

impl AtomicWrite {
    /// The Entity types this write's Temporal Evolution entries are recorded
    /// under.
    ///
    /// Clause 4.5.6 makes the Temporal Evolution of an Entity "the Entity
    /// [...] as mandated by clause 4.5.1", and clause 4.5.1 makes at least one
    /// Entity type part of what an Entity is, so recording an observation
    /// states what the Entity is and cannot be done without knowing it.
    ///
    /// A write that states current Entity state carries the whole Entity the
    /// mutation produced, so the Entity's types are the ones its record
    /// already extracted. They are read from there rather than from the
    /// request fragment the mutation was computed from: clauses 5.6.2.4,
    /// 5.6.3.4 and 5.6.17.4 all take an Entity Fragment whose `type` is
    /// optional — each says only "If type is included in the Fragment" — so a
    /// fragment routinely names no type at all while the Entity it was merged
    /// into names several.
    ///
    /// A write that states no current Entity state has only its own
    /// observation to read. Clauses 5.6.11 and 5.6.12 let a Temporal Evolution
    /// be provided without current state ever existing, and the payload of
    /// those operations is an `EntityTemporal` (clause 5.2.20), which is
    /// "the same datatype as mandated by clause 5.2.4" and therefore carries
    /// `type`.
    #[must_use]
    pub fn recorded_entity_types(&self) -> Vec<EntityTypeName> {
        match self.entity.record() {
            Some(record) => record.projection.types.clone(), // clone: the durable temporal intent outlives the write it was interpreted from.
            None => self
                .history
                .iter()
                .find_map(|consequence| match consequence {
                    HistoryRecord::Observations(payload) => Some(extract_types(payload)),
                    HistoryRecord::Tombstone(_) | HistoryRecord::ScopeRemoval => None,
                })
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        entity_record::EntityRecord,
        history_record::{HistoryRecord, tombstone},
        precondition::Precondition,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName, uri::Uri};
    use serde_json::{Value, json};
    use std::sync::Arc;

    const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";

    fn entity_id() -> Arc<Uri> {
        Arc::new("urn:ngsi-ld:Vehicle:1".parse::<Uri>().expect("valid entity URI"))
    }

    /// The merged Entity a write stores back, carrying its clause 4.5.1 types.
    fn merged_entity() -> Arc<Value> {
        Arc::new(json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": [VEHICLE],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
            }]
        }]))
    }

    /// The Entity Fragment a clause 5.6.3 Append Attributes request carries:
    /// one attribute and, as clause 5.6.3.4 permits, no `type` at all.
    fn typeless_fragment() -> Arc<Value> {
        Arc::new(json!([{
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 42}]
            }]
        }]))
    }

    fn named(types: &[EntityTypeName]) -> Vec<&str> {
        types.iter().map(EntityTypeName::as_str).collect()
    }

    #[test]
    fn the_builder_defaults_every_optional_consequence() {
        let write = AtomicWrite::builder().entity_id(entity_id()).entity(EntityMutation::None).build();
        assert!(write.history.is_empty());
        assert!(write.subscription_matches.is_empty());
        assert!(write.side_effects.is_none());
        pretty_assertions::assert_eq!(write.precondition, Precondition::None);
    }

    /// Clause 5.6.3.4 says only "If type is included in the Fragment", so an
    /// Append Attributes fragment routinely names no type while the Entity it
    /// was merged into names one. The types recorded are the merged Entity's.
    #[test]
    fn a_write_that_stores_entity_state_records_the_merged_entitys_types_not_the_fragments() {
        let write = AtomicWrite::builder()
            .entity_id(entity_id())
            .entity(EntityMutation::Update(EntityRecord::from_expanded(merged_entity())))
            .history(vec![HistoryRecord::Observations(typeless_fragment())])
            .build();
        pretty_assertions::assert_eq!(named(&write.recorded_entity_types()), vec![VEHICLE]);
    }

    /// Clauses 5.6.11 and 5.6.12 provide a Temporal Evolution without current
    /// state, so a write that stores no Entity row reads its types from the
    /// `EntityTemporal` payload of clause 5.2.20, which carries `type`.
    #[test]
    fn a_write_that_stores_no_entity_state_records_the_observed_payloads_types() {
        let write = AtomicWrite::builder()
            .entity_id(entity_id())
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(merged_entity())])
            .build();
        pretty_assertions::assert_eq!(named(&write.recorded_entity_types()), vec![VEHICLE]);
    }

    /// An Entity deletion stores no Entity row and observes nothing: clauses
    /// 4.5.7 and 4.5.8 give it only endings to record, and an ending names no
    /// Entity type.
    #[test]
    fn an_entity_deletion_records_no_entity_types() {
        let speed = "https://example.com/speed".parse::<AttributeName>().expect("valid attribute name");
        let write = AtomicWrite::builder()
            .entity_id(entity_id())
            .entity(EntityMutation::Delete)
            .history(vec![tombstone(&speed, None, AttributeType::Property)])
            .build();
        pretty_assertions::assert_eq!(write.recorded_entity_types(), Vec::new());
    }
}
