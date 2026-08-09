//! Turning the NGSI-LD side of an atomic write into PostgreSQL bind columns.
//!
//! Everything the store does to a value on its way to a column happens here:
//! rendering a clause 4.18 scope as an `ltree` path, serialising the expanded
//! payload once for the wire, and encoding the hoisted geometries as EWKB. That
//! is what lets [`EntityRecord`] stay in specification terms.
//!
//! [`EntityRecord`]: crate::service::entity::entity_mutation::EntityRecord

use crate::{error::AndromedaError, repository::atomic_write::entity_group::IndexedWrite};
use andromeda_postgresql_context_store::entity::params::PreparedEntityColumns;
use andromeda_postgresql_query::ltree::scope_to_ltree;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Build the parallel bind vectors for one fused entity statement.
///
/// # Errors
///
/// Returns [`AndromedaError::internal`] if a grouped write carries no entity
/// record, which the grouping in [`group`](crate::repository::atomic_write::entity_group::group)
/// makes impossible, or if a payload or geometry cannot be encoded.
pub(super) fn columns_for(writes: &[IndexedWrite<'_>]) -> Result<PreparedEntityColumns, AndromedaError> {
    let mut columns = PreparedEntityColumns::with_capacity(writes.len());
    for (_, write) in writes {
        let record = write
            .entity
            .record()
            .ok_or_else(|| AndromedaError::internal(format!("atomic write for {} carries no entity record", write.entity_id)))?;

        columns.ids.push(write.entity_id.as_ref().clone()); // clone: the SQL column set owns identifiers borrowed from the writes.
        columns.types_json.push(Value::Array(
            record.types.iter().map(|entity_type| Value::String(entity_type.to_string())).collect(),
        ));
        columns
            .scope_paths_json
            .push(Value::Array(record.scopes.iter().map(|scope| Value::String(scope_to_ltree(scope))).collect()));
        // clone: the payload is shared with the change-event path, and the bind
        // vector owns a `Value`; the serialised form below is what reaches the wire.
        columns.push_payload(Value::clone(&record.payload))?;
        columns.push_geometries(record.location.as_ref(), record.observation_space.as_ref(), record.operation_space.as_ref())?;
        columns.expires_ats.push(record.expires_at);
    }
    Ok(columns)
}

/// The `modified_at` each update expects to find, `None` where it expects
/// nothing.
pub(super) fn expectations(writes: &[IndexedWrite<'_>]) -> Vec<Option<DateTime<Utc>>> {
    writes.iter().map(|(_, write)| write.precondition.expected_modified_at()).collect()
}

#[cfg(test)]
mod tests {
    use super::{columns_for, expectations};
    use crate::service::entity::{
        atomic_write::AtomicWrite,
        entity_mutation::{EntityMutation, EntityRecord},
        precondition::Precondition,
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_lexicon::{scope::grammar::ScopeBuf, uri::Uri};
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Arc;

    fn scoped_write(id: &str, scopes: Vec<&str>, location: Option<Geometry>) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Uri>().expect("valid entity URI")))
            .entity(EntityMutation::Update(EntityRecord {
                payload: Arc::new(json!([{"@id": id, "https://example.org/speed": [{"@value": 1}]}])),
                types: Vec::new(),
                scopes: scopes.into_iter().map(|raw| ScopeBuf::new(raw.to_string()).expect("scope")).collect(),
                location,
                observation_space: None,
                operation_space: None,
                expires_at: None,
            }))
            .build()
    }

    fn point() -> Geometry {
        serde_json::from_value(json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("point")
    }

    #[test]
    fn scopes_render_as_ltree_paths() {
        let writes = [scoped_write("urn:ngsi-ld:A", vec!["/a/b", "/c"], None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();

        let columns = columns_for(&indexed).expect("columns");

        pretty_assertions::assert_eq!(columns.scope_paths_json[0], json!(["a.b", "c"]));
    }

    #[test]
    fn the_payload_is_serialised_once_into_both_vectors() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None), scoped_write("urn:ngsi-ld:B", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();

        let columns = columns_for(&indexed).expect("columns");

        pretty_assertions::assert_eq!(columns.payloads.len(), 2);
        pretty_assertions::assert_eq!(columns.payload_json.len(), 2);
        for (payload, json) in columns.payloads.iter().zip(&columns.payload_json) {
            let round_tripped: serde_json::Value = serde_json::from_str(json).expect("payload json parses");
            pretty_assertions::assert_eq!(&round_tripped, payload);
        }
    }

    #[test]
    fn an_absent_geometry_binds_null() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();

        let columns = columns_for(&indexed).expect("columns");

        assert!(columns.locations[0].is_none());
    }

    #[test]
    fn every_column_vector_has_one_entry_per_write() {
        let writes = [
            scoped_write("urn:ngsi-ld:A", vec!["/a"], Some(point())),
            scoped_write("urn:ngsi-ld:B", Vec::new(), None),
            scoped_write("urn:ngsi-ld:C", vec!["/c"], Some(point())),
            scoped_write("urn:ngsi-ld:D", Vec::new(), Some(point())),
            scoped_write("urn:ngsi-ld:E", Vec::new(), None),
        ];
        let indexed: Vec<_> = writes.iter().enumerate().collect();

        let columns = columns_for(&indexed).expect("columns");

        for length in [
            columns.ids.len(),
            columns.types_json.len(),
            columns.scope_paths_json.len(),
            columns.payloads.len(),
            columns.payload_json.len(),
            columns.locations.len(),
            columns.obs_spaces.len(),
            columns.op_spaces.len(),
            columns.expires_ats.len(),
        ] {
            pretty_assertions::assert_eq!(length, 5);
        }
    }

    #[test]
    fn only_an_expectant_precondition_contributes_a_timestamp() {
        let now = Utc::now();
        let writes = [
            AtomicWrite::builder()
                .entity_id(Arc::new("urn:ngsi-ld:A".parse::<Uri>().expect("valid entity URI")))
                .entity(EntityMutation::Delete)
                .precondition(Precondition::UnchangedSince(now))
                .build(),
            AtomicWrite::builder()
                .entity_id(Arc::new("urn:ngsi-ld:B".parse::<Uri>().expect("valid entity URI")))
                .entity(EntityMutation::Delete)
                .precondition(Precondition::MustExist)
                .build(),
        ];
        let indexed: Vec<_> = writes.iter().enumerate().collect();

        pretty_assertions::assert_eq!(expectations(&indexed), vec![Some(now), None]);
    }
}
