//! Encoding semantic entity records into PostgreSQL bind columns.

use crate::{entity_group::IndexedWrite, name_resolution::ResolvedWriteNames};
use andromeda_context_commit::{
    commit_invariant::{CommitInvariant, NameDictionary},
    error::CommitError,
    projected_attributes::ProjectedAttributes,
};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, attribute_kind_code::stored_attribute_kind};
use andromeda_postgresql_context_store::entity::params::PreparedEntityColumns;
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, ltree::scope_to_ltree};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::HashMap, hash::Hash};

/// Build parallel bind vectors for one fused entity statement.
///
/// `names` is the resolution of every type and attribute name the writes
/// project; one it does not hold is an invariant failure, because the keys
/// were minted from these very writes in this transaction.
pub(crate) fn columns_for(writes: &[IndexedWrite<'_>], names: &ResolvedWriteNames) -> Result<PreparedEntityColumns, CommitError> {
    let mut columns = PreparedEntityColumns::with_capacity(writes.len());
    for (_, write) in writes {
        let record = write.entity.record().ok_or_else(|| {
            CommitError::from(CommitInvariant::MissingEntityRecord {
                entity: write.entity_id.as_ref().clone(), // clone: the error owns the entity it names
            })
        })?;
        let projection = &record.projection;
        columns.ids.push(write.entity_id.as_ref().clone()); // clone: the SQL column set owns identifiers borrowed from the writes.
        columns.push_entity_type_name_keys(key_array(&projection.types, &names.types, NameDictionary::EntityType)?);
        let attribute_keys = key_array(projection.attributes.names(), &names.attributes, NameDictionary::AttributeName)?;
        columns.push_attribute_instances(instance_keys(&projection.attributes, &attribute_keys));
        columns.push_attribute_name_keys(attribute_keys);
        columns.scope_paths_json.push(Value::Array(
            projection.scopes.iter().map(|scope| Value::String(scope_to_ltree(scope))).collect(),
        ));
        columns
            .push_payload(&record.payload)
            .map_err(|error| CommitError::Persistence(PersistenceFailure::Unencodable.record(&error)))?;
        columns
            .push_geometries(
                projection.location.as_ref(),
                projection.observation_space.as_ref(),
                projection.operation_space.as_ref(),
            )
            .map_err(|error| CommitError::Persistence(PersistenceFailure::Unencodable.record(&error)))?;
        columns.location_expires_ats.push(projection.location_expires_at);
        columns.expires_ats.push(record.expires_at);
        columns.min_attribute_expires_ats.push(record.min_attribute_expires_at);
    }
    Ok(columns)
}

/// The JSON array of dictionary keys for `names`, in declaration order.
///
/// `dictionary` names the dictionary in the invariant failure a missing key
/// raises, so the failure says which dictionary broke its promise.
fn key_array<N, K>(names: &[N], keys: &HashMap<N, K>, dictionary: NameDictionary) -> Result<Vec<K>, CommitError>
where
    N: Eq + Hash,
    K: Copy,
{
    names
        .iter()
        .map(|name| {
            keys.get(name)
                .copied()
                .ok_or(CommitError::Invariant(CommitInvariant::UnresolvedName(dictionary)))
        })
        .collect()
}

/// The dictionary key and stored family code of every attribute instance:
/// each instance takes the key of the member that holds it, `keys` being the
/// members' keys in member order.
fn instance_keys(attributes: &ProjectedAttributes, keys: &[AttributeNameKey]) -> Vec<(AttributeNameKey, AttributeKind)> {
    attributes
        .members()
        .zip(keys)
        .flat_map(|((_, families), key)| families.iter().map(move |family| (*key, stored_attribute_kind(*family))))
        .collect()
}

/// Return update/delete version expectations aligned with grouped writes.
pub(crate) fn expectations(writes: &[IndexedWrite<'_>]) -> Vec<Option<DateTime<Utc>>> {
    writes.iter().map(|(_, write)| write.precondition.expected_modified_at()).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        columns::{CommitInvariant, NameDictionary, columns_for, expectations},
        name_resolution::ResolvedWriteNames,
    };
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        entity_record::{EntityRecord, SearchProjection},
        error::CommitError,
        precondition::Precondition,
        projected_attributes::ProjectedAttributes,
    };
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        entity_type_name::EntityTypeName,
        identifier::Identifier,
        scope::path::ScopePath,
    };
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
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
            types: HashMap::from([(vehicle(), EntityTypeNameKey::from(7))]),
            attributes: HashMap::from([(speed(), AttributeNameKey::from(11))]),
        }
    }

    /// The member `speed`, holding one Property instance.
    fn speed_attribute() -> ProjectedAttributes {
        let mut attributes = ProjectedAttributes::default();
        attributes.push(speed(), |_| [AttributeType::Property]);
        attributes
    }

    fn scoped_write(id: &str, scopes: Vec<&str>, location: Option<Geometry>) -> AtomicWrite {
        let projection = SearchProjection {
            types: vec![vehicle()],
            attributes: speed_attribute(),
            scopes: scopes.into_iter().map(|raw| raw.parse::<ScopePath>().expect("valid scope path")).collect(),
            location,
            observation_space: None,
            operation_space: None,
            location_expires_at: None,
        };
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Identifier>().expect("valid entity URI")))
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
        pretty_assertions::assert_eq!(columns.entity_type_name_keys[0].as_str(), "{7}");
        pretty_assertions::assert_eq!(columns.attribute_name_keys[0].as_str(), "{11}");
        pretty_assertions::assert_eq!(columns.attribute_instance_name_keys[0].as_str(), "{11}");
        pretty_assertions::assert_eq!(columns.attribute_instance_kinds[0].as_str(), "{1}", "a Property is stored as family code 1");
    }

    /// The stored instance arrays are compared whole to decide whether a write
    /// rewrites the search row, so they are encoded in one canonical order
    /// whatever order the payload listed its members in.
    #[test]
    fn attribute_instances_encode_in_one_canonical_order() {
        let colour: AttributeName = "https://example.org/colour".parse().expect("valid attribute name");
        let names = ResolvedWriteNames {
            types: resolved().types,
            attributes: HashMap::from([(speed(), AttributeNameKey::from(11)), (colour.clone(), AttributeNameKey::from(3))]),
        };
        let encode = |attributes: ProjectedAttributes| {
            let mut write = scoped_write("urn:ngsi-ld:A", Vec::new(), None);
            if let EntityMutation::Update(record) = &mut write.entity {
                record.projection.attributes = attributes;
            }
            let writes = [write];
            let indexed: Vec<_> = writes.iter().enumerate().collect();
            let columns = columns_for(&indexed, &names).expect("valid columns");
            (
                columns.attribute_instance_name_keys[0].as_str().to_owned(),
                columns.attribute_instance_kinds[0].as_str().to_owned(),
            )
        };
        let mut forward_members = ProjectedAttributes::default();
        forward_members.push(speed(), |_| [AttributeType::Relationship, AttributeType::Property]);
        forward_members.push(colour.clone(), |_| [AttributeType::Property]);
        let mut backward_members = ProjectedAttributes::default();
        backward_members.push(colour, |_| [AttributeType::Property]);
        backward_members.push(speed(), |_| [AttributeType::Property, AttributeType::Relationship]);
        let forward = encode(forward_members);
        let backward = encode(backward_members);
        pretty_assertions::assert_eq!(forward, ("{3,11,11}".to_owned(), "{1,1,2}".to_owned()));
        pretty_assertions::assert_eq!(forward, backward);
    }

    /// A member that holds no attribute instance, like `scope`, is keyed for
    /// `attrs` and contributes nothing to the instance arrays.
    #[test]
    fn a_member_without_instances_is_keyed_but_not_counted() {
        let scope: AttributeName = "https://uri.etsi.org/ngsi-ld/scope".parse().expect("valid attribute name");
        let names = ResolvedWriteNames {
            types: resolved().types,
            attributes: HashMap::from([(speed(), AttributeNameKey::from(11)), (scope.clone(), AttributeNameKey::from(5))]),
        };
        let mut write = scoped_write("urn:ngsi-ld:A", Vec::new(), None);
        if let EntityMutation::Update(record) = &mut write.entity {
            record.projection.attributes.push(scope, |_| []);
        }
        let writes = [write];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &names).expect("valid columns");
        pretty_assertions::assert_eq!(columns.attribute_name_keys[0].as_str(), "{11,5}");
        pretty_assertions::assert_eq!(columns.attribute_instance_name_keys[0].as_str(), "{11}");
    }

    /// The keys were minted from these very writes, so a name without one is
    /// a broken invariant rather than a value to default.
    #[test]
    fn a_name_the_dictionary_did_not_resolve_is_an_invariant_failure() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let refused = columns_for(&indexed, &ResolvedWriteNames::default()).expect_err("an unresolved name is refused");
        assert!(matches!(refused, CommitError::Invariant(_)), "{refused:?}");
        let types_only = ResolvedWriteNames {
            types: resolved().types,
            attributes: HashMap::new(),
        };
        let refused = columns_for(&indexed, &types_only).expect_err("an unresolved attribute name is refused");
        assert!(
            matches!(refused, CommitError::Invariant(CommitInvariant::UnresolvedName(NameDictionary::AttributeName))),
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
            columns.entity_type_name_keys.len(),
            columns.attribute_name_keys.len(),
            columns.attribute_instance_name_keys.len(),
            columns.attribute_instance_kinds.len(),
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
                .entity_id(Arc::new("urn:ngsi-ld:A".parse::<Identifier>().expect("valid entity URI")))
                .entity(EntityMutation::Delete)
                .precondition(Precondition::UnchangedSince(now))
                .build(),
            AtomicWrite::builder()
                .entity_id(Arc::new("urn:ngsi-ld:B".parse::<Identifier>().expect("valid entity URI")))
                .entity(EntityMutation::Delete)
                .precondition(Precondition::MustExist)
                .build(),
        ];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        pretty_assertions::assert_eq!(expectations(&indexed), vec![Some(now), None]);
    }
}
