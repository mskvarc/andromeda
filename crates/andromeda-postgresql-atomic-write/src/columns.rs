//! Encoding semantic entity records into PostgreSQL bind columns.

use crate::{commit_refusal::persistence, entity_group::IndexedWrite};
use andromeda_context_commit::error::CommitError;
use andromeda_postgresql_context_store::entity::params::PreparedEntityColumns;
use andromeda_postgresql_query::ltree::scope_to_ltree;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Build parallel bind vectors for one fused entity statement.
pub(crate) fn columns_for(writes: &[IndexedWrite<'_>]) -> Result<PreparedEntityColumns, CommitError> {
    let mut columns = PreparedEntityColumns::with_capacity(writes.len());
    for (_, write) in writes {
        let record = write.entity.record().ok_or_else(|| CommitError::Invariant {
            detail: format!("atomic write for {} carries no entity record", write.entity_id),
        })?;
        columns.ids.push(write.entity_id.as_ref().clone()); // clone: the SQL column set owns identifiers borrowed from the writes.
        columns.types_json.push(Value::Array(
            record.types.iter().map(|entity_type| Value::String(entity_type.to_string())).collect(),
        ));
        columns
            .scope_paths_json
            .push(Value::Array(record.scopes.iter().map(|scope| Value::String(scope_to_ltree(scope))).collect()));
        columns.push_payload(Value::clone(&record.payload)).map_err(persistence)?;
        columns
            .push_geometries(record.location.as_ref(), record.observation_space.as_ref(), record.operation_space.as_ref())
            .map_err(persistence)?;
        columns.expires_ats.push(record.expires_at);
    }
    Ok(columns)
}

/// Return update/delete version expectations aligned with grouped writes.
pub(crate) fn expectations(writes: &[IndexedWrite<'_>]) -> Vec<Option<DateTime<Utc>>> {
    writes.iter().map(|(_, write)| write.precondition.expected_modified_at()).collect()
}

#[cfg(test)]
mod tests {
    use crate::columns::{columns_for, expectations};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::{EntityMutation, EntityRecord},
        precondition::Precondition,
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_lexicon::{scope::grammar::ScopeBuf, uri::Uri};
    use chrono::Utc;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn scoped_write(id: &str, scopes: Vec<&str>, location: Option<Geometry>) -> AtomicWrite {
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Uri>().expect("valid entity URI")))
            .entity(EntityMutation::Update(EntityRecord {
                payload: Arc::new(json!([{"@id": id, "https://example.org/speed": [{"@value": 1}]}])),
                types: Vec::new(),
                scopes: scopes.into_iter().map(|raw| ScopeBuf::new(raw.to_string()).expect("valid scope")).collect(),
                location,
                observation_space: None,
                operation_space: None,
                expires_at: None,
            }))
            .build()
    }

    fn point() -> Geometry {
        serde_json::from_value(json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("valid point")
    }

    #[test]
    fn scopes_are_encoded_as_ltree_paths() {
        let writes = [scoped_write("urn:ngsi-ld:A", vec!["/a/b", "/c"], None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed).expect("valid columns");
        pretty_assertions::assert_eq!(columns.scope_paths_json[0], json!(["a.b", "c"]));
    }

    #[test]
    fn payloads_are_prepared_for_both_bind_strategies() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None), scoped_write("urn:ngsi-ld:B", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed).expect("valid columns");
        pretty_assertions::assert_eq!(columns.payloads.len(), 2);
        pretty_assertions::assert_eq!(columns.payload_json.len(), 2);
        for (payload, encoded) in columns.payloads.iter().zip(&columns.payload_json) {
            let round_tripped: Value = serde_json::from_str(encoded).expect("valid payload JSON");
            pretty_assertions::assert_eq!(&round_tripped, payload);
        }
    }

    #[test]
    fn every_bind_column_remains_positionally_aligned() {
        let writes = [
            scoped_write("urn:ngsi-ld:A", vec!["/a"], Some(point())),
            scoped_write("urn:ngsi-ld:B", Vec::new(), None),
            scoped_write("urn:ngsi-ld:C", vec!["/c"], Some(point())),
        ];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed).expect("valid columns");
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
            pretty_assertions::assert_eq!(length, 3);
        }
    }

    #[test]
    fn only_versioned_preconditions_bind_an_expected_timestamp() {
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
