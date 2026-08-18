//! Encoding semantic entity records into PostgreSQL bind columns.

use crate::{commit_refusal::persistence, entity_group::IndexedWrite, name_resolution::ResolvedWriteNames};
use andromeda_context_commit::error::CommitError;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_context_store::entity::params::PreparedEntityColumns;
use andromeda_postgresql_query::ltree::scope_to_ltree;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashMap, fmt::Display, hash::Hash};

/// Build parallel bind vectors for one fused entity statement.
///
/// `names` is the resolution of every type and attribute name the writes
/// project; one it does not hold is an invariant failure, because the keys
/// were minted from these very writes in this transaction.
pub(crate) fn columns_for(writes: &[IndexedWrite<'_>], names: &ResolvedWriteNames) -> Result<PreparedEntityColumns, CommitError> {
    let mut columns = PreparedEntityColumns::with_capacity(writes.len());
    for (_, write) in writes {
        let record = write.entity.record().ok_or_else(|| CommitError::Invariant {
            detail: format!("atomic write for {} carries no entity record", write.entity_id),
        })?;
        let projection = &record.projection;
        columns.ids.push(write.entity_id.as_ref().clone()); // clone: the SQL column set owns identifiers borrowed from the writes.
        columns
            .type_ids_json
            .push(key_array(&projection.types, &names.types, |key| key.get(), "entity type", &write.entity_id)?);
        columns.attribute_name_ids_json.push(key_array(
            &projection.attribute_names,
            &names.attributes,
            |key| key.get(),
            "attribute name",
            &write.entity_id,
        )?);
        columns.scope_paths_json.push(Value::Array(
            projection.scopes.iter().map(|scope| Value::String(scope_to_ltree(scope))).collect(),
        ));
        columns.push_payload(&record.payload).map_err(persistence)?;
        columns
            .push_geometries(
                projection.location.as_ref(),
                projection.observation_space.as_ref(),
                projection.operation_space.as_ref(),
            )
            .map_err(persistence)?;
        columns.location_expires_ats.push(projection.location_expires_at);
        columns.expires_ats.push(record.expires_at);
        columns.min_attribute_expires_ats.push(record.min_attribute_expires_at);
    }
    Ok(columns)
}

/// The JSON array of dictionary keys for `names`, in declaration order.
///
/// `dictionary` describes the dictionary in the invariant failure a missing
/// key raises, so the message names which dictionary broke its promise.
fn key_array<N, K>(names: &[N], keys: &HashMap<N, K>, key_value: impl Fn(&K) -> i32, dictionary: &str, entity_id: &Uri) -> Result<Value, CommitError>
where
    N: Eq + Hash + Display,
{
    names
        .iter()
        .map(|name| {
            keys.get(name).map(|key| Value::from(key_value(key))).ok_or_else(|| CommitError::Invariant {
                detail: format!("{dictionary} dictionary returned no key for {name} on {entity_id}"),
            })
        })
        .collect::<Result<Vec<Value>, CommitError>>()
        .map(Value::Array)
}

/// Return update/delete version expectations aligned with grouped writes.
pub(crate) fn expectations(writes: &[IndexedWrite<'_>]) -> Vec<Option<DateTime<Utc>>> {
    writes.iter().map(|(_, write)| write.precondition.expected_modified_at()).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        columns::{columns_for, expectations},
        name_resolution::ResolvedWriteNames,
    };
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        entity_record::{EntityRecord, SearchProjection},
        error::CommitError,
        precondition::Precondition,
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::grammar::ScopeBuf, uri::Uri};
    use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, entity_type_id::EntityTypeId};
    use chrono::Utc;
    use serde_json::{Value, json};
    use std::{collections::HashMap, sync::Arc};

    const VEHICLE: &str = "https://example.org/Vehicle";
    const SPEED: &str = "https://example.org/speed";

    fn vehicle() -> EntityTypeName {
        VEHICLE.parse().expect("valid type name")
    }

    fn speed() -> AttributeName {
        SPEED.parse().expect("valid attribute name")
    }

    fn resolved() -> ResolvedWriteNames {
        ResolvedWriteNames {
            types: HashMap::from([(vehicle(), EntityTypeId::from(7))]),
            attributes: HashMap::from([(speed(), AttributeNameId::from(11))]),
        }
    }

    fn scoped_write(id: &str, scopes: Vec<&str>, location: Option<Geometry>) -> AtomicWrite {
        let projection = SearchProjection {
            types: vec![vehicle()],
            attribute_names: vec![speed()],
            scopes: scopes.into_iter().map(|raw| ScopeBuf::new(raw.to_string()).expect("valid scope")).collect(),
            location,
            observation_space: None,
            operation_space: None,
            location_expires_at: None,
        };
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Uri>().expect("valid entity URI")))
            .entity(EntityMutation::Update(EntityRecord::from_parts(
                Arc::new(json!([{"@id": id, SPEED: [{"@value": 1}]}])),
                projection,
                None,
                None,
            )))
            .build()
    }

    fn point() -> Geometry {
        serde_json::from_value(json!({"type": "Point", "coordinates": [1.0, 2.0]})).expect("valid point")
    }

    /// The key columns carry dictionary keys, never IRIs: the resolved key of
    /// every declared type and attribute name, in declaration order.
    #[test]
    fn types_and_attribute_names_are_encoded_as_their_dictionary_keys() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &resolved()).expect("valid columns");
        pretty_assertions::assert_eq!(columns.type_ids_json[0], json!([7]));
        pretty_assertions::assert_eq!(columns.attribute_name_ids_json[0], json!([11]));
    }

    /// The keys were minted from these very writes, so a name without one is
    /// a broken invariant rather than a value to default.
    #[test]
    fn a_name_the_dictionary_did_not_resolve_is_an_invariant_failure() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let refused = columns_for(&indexed, &ResolvedWriteNames::default()).expect_err("an unresolved name is refused");
        assert!(matches!(refused, CommitError::Invariant { .. }), "{refused:?}");
        let types_only = ResolvedWriteNames {
            types: resolved().types,
            attributes: HashMap::new(),
        };
        let refused = columns_for(&indexed, &types_only).expect_err("an unresolved attribute name is refused");
        assert!(
            matches!(refused, CommitError::Invariant { ref detail } if detail.contains("attribute name")),
            "{refused:?}"
        );
    }

    #[test]
    fn scopes_are_encoded_as_ltree_paths() {
        let writes = [scoped_write("urn:ngsi-ld:A", vec!["/a/b", "/c"], None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &resolved()).expect("valid columns");
        pretty_assertions::assert_eq!(columns.scope_paths_json[0], json!(["a.b", "c"]));
    }

    #[test]
    fn payloads_are_serialised_once_as_json_text() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None), scoped_write("urn:ngsi-ld:B", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &resolved()).expect("valid columns");
        pretty_assertions::assert_eq!(columns.payload_json.len(), 2);
        for ((_, write), encoded) in indexed.iter().zip(&columns.payload_json) {
            let round_tripped: Value = serde_json::from_str(encoded).expect("valid payload JSON");
            let record = write.entity.record().expect("an update carries a record");
            pretty_assertions::assert_eq!(&round_tripped, record.payload.as_ref());
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
        let columns = columns_for(&indexed, &resolved()).expect("valid columns");
        for length in [
            columns.ids.len(),
            columns.type_ids_json.len(),
            columns.attribute_name_ids_json.len(),
            columns.scope_paths_json.len(),
            columns.payload_json.len(),
            columns.locations.len(),
            columns.obs_spaces.len(),
            columns.op_spaces.len(),
            columns.location_expires_ats.len(),
            columns.expires_ats.len(),
            columns.min_attribute_expires_ats.len(),
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
