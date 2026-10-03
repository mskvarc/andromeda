//! A durable temporal intent reads back from its queue row whatever nesting a
//! write is admitted with.
//!
//! The intent holds the entity state the write observed one container deeper
//! than its row does, and each history row's value and metadata five deeper;
//! the queue decodes it with `serde_json`'s reader all the same.

use crate::append_intent::TemporalAppendIntent;
use andromeda_context_commit::{history_record::HistoryRecord, write_id::WriteId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_number::json_value::READER_NESTING_LIMIT;
use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
use andromeda_test_fixtures::nesting_fixture::{NestingShape, container_depth, nested_instance, stored_entity};
use chrono::Utc;
use std::sync::Arc;

/// At the deepest nesting any limit may admit, the intent recording each
/// nesting shape stays within the reader and decodes to the intent it was
/// written from.
#[test]
fn an_intent_recording_an_attribute_at_the_decodable_ceiling_reads_back() {
    let ceiling = AttributeNestingLimit::DECODABLE_CEILING.get();
    let entity_id: Identifier = "urn:ngsi-ld:NestingProbe:1".parse().expect("an entity identifier");
    let types = vec!["https://example.org/NestingProbe".parse::<EntityTypeName>().expect("an entity type")];
    for shape in NestingShape::ALL {
        let stored = serde_json::to_vec(&stored_entity(entity_id.as_str(), nested_instance(shape, ceiling))).expect("JSON");
        let state = Arc::new(EntityState::from_slice(&stored).expect("the state decodes"));
        let intent = TemporalAppendIntent::from_history(
            &entity_id,
            &types,
            &[HistoryRecord::Observations(state)],
            WriteRecord::restored(WriteId::mint(), Utc::now()),
            WriteOrdinal::at(0),
        )
        .expect("the history is recordable")
        .expect("the observation is an intent");
        let payload = serde_json::to_vec(&intent).expect("the intent encodes");
        assert!(
            container_depth(&payload) <= READER_NESTING_LIMIT,
            "{shape:?} nests {} containers",
            container_depth(&payload)
        );
        let decoded: TemporalAppendIntent = serde_json::from_slice(&payload).unwrap_or_else(|error| panic!("{shape:?}: {error}"));
        let reencoded = serde_json::to_vec(&decoded).expect("the decoded intent encodes");
        assert!(
            reencoded == payload,
            "{shape:?}: the decoded intent writes back other JSON than it was read from"
        );
    }
}
