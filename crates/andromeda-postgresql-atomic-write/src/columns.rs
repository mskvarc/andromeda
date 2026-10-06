//! Encoding semantic entity records into PostgreSQL bind columns.

use crate::{entity_group::IndexedWrite, name_resolution::ResolvedWriteNames};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_invariant::{CommitInvariant, NameDictionary},
    entity_mutation::EntityMutation,
    error::CommitError,
    hoisted_geometries::GeometryChange,
    precondition::Precondition,
    projected_attributes::ProjectedAttributes,
};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, attribute_kind_code::stored_attribute_kind};
use andromeda_postgresql_context_store::entity::{params::PreparedEntityColumns, scope_label_paths::ScopeLabelPaths, staged_geometry::StagedGeometry};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, ltree::StoredScopePath};
use chrono::{DateTime, Utc};
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
        let state = &record.state;
        let row = StoredRow::of(write);
        let changes = &record.geometry_changes;
        columns.ids.push(write.entity_id.as_ref().clone()); // clone: the SQL column set owns identifiers borrowed from the writes.
        columns.push_entity_type_name_keys(key_array(&projection.types, &names.types, NameDictionary::EntityType)?);
        let attribute_keys = key_array(projection.attributes.names(), &names.attributes, NameDictionary::AttributeName)?;
        columns.push_attribute_instances(instance_keys(&projection.attributes, &attribute_keys));
        columns.push_attribute_name_keys(attribute_keys);
        columns.scope_paths_json.push(ScopeLabelPaths::new(
            // `entity_search.scope_paths` is only matched against: the
            // Entity's Scope is read back from its stored document, which keeps
            // the written text, so the label path alone is stored here.
            projection.scopes.iter().map(|scope| StoredScopePath::from(scope).label_path).collect(),
        ));
        columns
            .push_payload(&record.state)
            .map_err(|error| CommitError::Persistence(PersistenceFailure::Unencodable.record(&error)))?;
        columns
            .push_geometries(
                row.staged(changes.location, state.location_geometry()),
                row.staged(changes.observation_space, state.observation_space_geometry()),
                row.staged(changes.operation_space, state.operation_space_geometry()),
            )
            .map_err(|error| CommitError::Persistence(PersistenceFailure::Unencodable.record(&error)))?;
        columns.location_expires_ats.push(projection.location_expires_at);
        columns.expires_ats.push(record.expires_at);
        columns.min_attribute_expires_ats.push(record.min_attribute_expires_at);
    }
    Ok(columns)
}

/// What a write knows of the search row it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoredRow {
    /// An update that holds against the version it read: if it lands, the
    /// search row it rewrites is the projection of the state it replaces.
    AsRead,
    /// An insert, or an update that overwrites whatever is stored.
    Unknown,
}

impl StoredRow {
    /// What `write` knows of the row it writes.
    const fn of(write: &AtomicWrite) -> StoredRow {
        match (&write.entity, write.precondition) {
            (EntityMutation::Update(_), Precondition::UnchangedSince(_)) => StoredRow::AsRead,
            (
                EntityMutation::Update(_) | EntityMutation::Insert(_) | EntityMutation::Delete | EntityMutation::None,
                Precondition::None | Precondition::MustExist | Precondition::MustNotExist | Precondition::UnchangedSince(_),
            ) => StoredRow::Unknown,
        }
    }

    /// How the row stages one hoisted geometry: kept when the stored row is
    /// the one the write read and the write leaves the geometry as that row
    /// holds it, encoded otherwise. A kept geometry costs the statement
    /// nothing in proportion to its positions.
    fn staged<G>(self, change: GeometryChange, geometry: Option<G>) -> StagedGeometry<G> {
        match (self, change) {
            (StoredRow::AsRead, GeometryChange::Unchanged) => StagedGeometry::Kept,
            (StoredRow::AsRead, GeometryChange::Changed) | (StoredRow::Unknown, GeometryChange::Changed | GeometryChange::Unchanged) => {
                StagedGeometry::Written(geometry)
            }
        }
    }
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
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        entity_type_name::EntityTypeName,
        identifier::Identifier,
        scope::path::ScopePath,
    };
    use andromeda_postgresql_context_store::entity::{scope_label_paths::ScopeLabelPaths, staged_geometry::StagedGeometry};
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
    use chrono::Utc;
    use serde_json::json;
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

    /// The default `location` of a test entity, as a longitude and latitude.
    type Located = Option<(f64, f64)>;

    /// A typed entity state, conveying a `location` Point when `location` is set.
    fn vehicle_state(id: &str, location: Located) -> EntityState {
        let Some((longitude, latitude)) = location else {
            return EntityState::new(id.parse().expect("valid entity URI"), EntityTypes::one(vehicle()));
        };
        let document = json!([{
            "@id": id,
            "@type": [VEHICLE],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]
                }]
            }]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    fn scoped_write(id: &str, scopes: Vec<&str>, location: Located) -> AtomicWrite {
        let projection = SearchProjection {
            types: vec![vehicle()],
            attributes: speed_attribute(),
            scopes: scopes.into_iter().map(|raw| raw.parse::<ScopePath>().expect("valid scope path")).collect(),
            location_expires_at: None,
        };
        AtomicWrite::builder()
            .entity_id(Arc::new(id.parse::<Identifier>().expect("valid entity URI")))
            .entity(EntityMutation::Update(EntityRecord::from_parts(
                Arc::new(vehicle_state(id, location)),
                projection,
                None,
                None,
            )))
            .build()
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
        pretty_assertions::assert_eq!(columns.scope_paths_json[0], ScopeLabelPaths::new(vec!["a.b".to_owned(), "c".to_owned()]));
    }

    /// Each state is encoded once, by the entity state codec, into the stored
    /// document it decodes back from.
    #[test]
    fn payloads_are_encoded_once_as_the_stored_document() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None), scoped_write("urn:ngsi-ld:B", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &resolved()).expect("valid columns");
        pretty_assertions::assert_eq!(columns.payload_json.len(), 2);
        for ((_, write), encoded) in indexed.iter().zip(&columns.payload_json) {
            let record = write.entity.record().expect("an update carries a record");
            pretty_assertions::assert_eq!(encoded.as_bytes(), record.state.to_vec().expect("the state encodes").as_slice());
            let decoded = EntityState::from_slice(encoded.as_bytes()).expect("the stored document decodes");
            pretty_assertions::assert_eq!(&decoded, record.state.as_ref());
        }
    }

    #[test]
    fn every_bind_column_remains_positionally_aligned() {
        let writes = [
            scoped_write("urn:ngsi-ld:A", vec!["/a"], Some((1.0, 2.0))),
            scoped_write("urn:ngsi-ld:B", Vec::new(), None),
            scoped_write("urn:ngsi-ld:C", vec!["/c"], Some((1.0, 2.0))),
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
        pretty_assertions::assert_eq!(
            columns.locations.iter().map(|staged| staged.ewkb().is_some()).collect::<Vec<_>>(),
            vec![true, false, true],
            "the location is encoded from the state the record shares"
        );
    }

    /// An update of the state it read, whose record shares that state's
    /// location, replacing it under `precondition`.
    fn update_sharing_the_location(precondition: Precondition) -> AtomicWrite {
        let previous = vehicle_state("urn:ngsi-ld:A", Some((13.4, 52.5)));
        let current = Arc::new(EntityState::clone(&previous)); // clone: shares every attribute, as a read-modify-write does
        AtomicWrite::builder()
            .entity_id(Arc::new("urn:ngsi-ld:A".parse::<Identifier>().expect("valid entity URI")))
            .entity(EntityMutation::Update(EntityRecord::replacing(&previous, current).expect("a projection")))
            .precondition(precondition)
            .build()
    }

    /// The resolution of the Vehicle type and of `location`.
    fn located_names() -> ResolvedWriteNames {
        let location: AttributeName = "https://uri.etsi.org/ngsi-ld/location".parse().expect("valid attribute name");
        ResolvedWriteNames {
            types: resolved().types,
            attributes: HashMap::from([(location, AttributeNameKey::from(13))]),
        }
    }

    /// Clause 5.6.2: an update that leaves `location` as the state it read held
    /// it, and holds against that version, stages the geometry as kept -- no
    /// EWKB is encoded or bound, so the statement never re-projects it.
    #[test]
    fn an_update_holding_against_its_read_keeps_an_unchanged_geometry() {
        let writes = [update_sharing_the_location(Precondition::UnchangedSince(Utc::now()))];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &located_names()).expect("valid columns");
        pretty_assertions::assert_eq!(columns.locations, vec![StagedGeometry::Kept]);
        pretty_assertions::assert_eq!(columns.obs_spaces, vec![StagedGeometry::Kept], "neither state holds one");
    }

    /// An update that overwrites whatever is stored cannot know the stored row
    /// holds the geometry it read, so it writes the geometry.
    #[test]
    fn an_unconditional_update_writes_even_an_unchanged_geometry() {
        let writes = [update_sharing_the_location(Precondition::MustExist)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &located_names()).expect("valid columns");
        assert!(columns.locations[0].ewkb().is_some(), "{:?}", columns.locations);
        pretty_assertions::assert_eq!(columns.obs_spaces, vec![StagedGeometry::Written(None)]);
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
