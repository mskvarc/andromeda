//! Encoding semantic entity records into PostgreSQL bind columns.

use crate::entity_group::IndexedWrite;
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
use andromeda_postgresql_attribute_instance::{flatten::FlattenError, name_keys::NameKeys};
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, attribute_kind_code::stored_attribute_kind};
use andromeda_postgresql_context_store::{
    current_attribute::{row_edit::RowEdit, staged_grains::StagedGrains, staged_rows::StagedRows},
    entity::{params::PreparedEntityColumns, staged_geometry::StagedGeometry, staged_scopes::StagedScopes},
};
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use chrono::{DateTime, Utc};

/// The binds of one fused entity statement: one entry per entity, and the
/// `current_attributes` rows and grains each entity's edit writes.
#[derive(Debug)]
pub(crate) struct EntityBinds<'a> {
    /// The parallel entity columns.
    pub(crate) entities: PreparedEntityColumns,
    /// The rows written whole, each naming its entity.
    pub(crate) rows: StagedRows<'a>,
    /// The grains an update moves or removes, and the entities it overwrites.
    pub(crate) grains: StagedGrains<'a>,
}

/// Build the binds of one fused entity statement.
///
/// `keys` is the resolution of every name the writes project or store; one it
/// does not hold is an invariant failure, because the keys were minted from
/// these very writes in this transaction. An update that states the state it
/// replaces stages only what differs from it ([`RowEdit`]); an insert, or an
/// update that read nothing, stages every instance.
pub(crate) fn columns_for<'a>(writes: &[IndexedWrite<'a>], keys: &NameKeys) -> Result<EntityBinds<'a>, CommitError> {
    let mut columns = PreparedEntityColumns::with_capacity(writes.len());
    let mut rows = StagedRows::default();
    let mut grains = StagedGrains::default();
    for &(_, write) in writes {
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
        columns.push_entity_type_name_keys(key_array(&projection.types, |name| keys.entity_type(name), NameDictionary::EntityType)?);
        let attribute_keys = key_array(projection.attributes.names(), |name| keys.attribute(name), NameDictionary::AttributeName)?;
        columns.push_attribute_instances(instance_keys(&projection.attributes, &attribute_keys));
        columns.push_attribute_name_keys(attribute_keys);
        columns.scopes.push(StagedScopes::of(&projection.scopes));
        let edit = RowEdit::between(record.replaces.as_deref(), state).map_err(|error| flatten_failure(&error))?;
        rows.push(write.entity_id.as_ref(), edit.rows(keys).map_err(|error| flatten_failure(&error))?)
            .map_err(|error| CommitError::Persistence(PersistenceFailure::Unencodable.record(&error)))?;
        grains.push(write.entity_id.as_ref(), &edit, keys).map_err(|error| flatten_failure(&error))?;
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
    Ok(EntityBinds {
        entities: columns,
        rows,
        grains,
    })
}

/// The commit failure a state that does not flatten into rows is.
///
/// A name without a key is the dictionaries breaking their promise; any other
/// refusal is a state the write did not settle, which the rows cannot hold.
fn flatten_failure(error: &FlattenError) -> CommitError {
    match error {
        FlattenError::UnresolvedAttribute(_) => CommitError::Invariant(CommitInvariant::UnresolvedName(NameDictionary::AttributeName)),
        FlattenError::UnresolvedType(_) => CommitError::Invariant(CommitInvariant::UnresolvedName(NameDictionary::EntityType)),
        FlattenError::Unstamped(_)
        | FlattenError::UncommittedSpelling(_)
        | FlattenError::SeveralJsonLiterals(_)
        | FlattenError::TooManyInstances(_)
        | FlattenError::Json(_) => CommitError::Persistence(PersistenceFailure::Unencodable.record(error)),
    }
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

/// The dictionary keys `key` gives for `names`, in declaration order.
///
/// `dictionary` names the dictionary in the invariant failure a missing key
/// raises, so the failure says which dictionary broke its promise.
fn key_array<N, K>(names: &[N], key: impl Fn(&N) -> Option<K>, dictionary: NameDictionary) -> Result<Vec<K>, CommitError> {
    names
        .iter()
        .map(|name| key(name).ok_or(CommitError::Invariant(CommitInvariant::UnresolvedName(dictionary))))
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
    use crate::columns::{CommitInvariant, NameDictionary, columns_for, expectations};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        entity_mutation::EntityMutation,
        entity_record::{EntityRecord, SearchProjection},
        error::CommitError,
        precondition::Precondition,
        projected_attributes::ProjectedAttributes,
    };
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, entity_types::EntityTypes, system_instant::SystemInstant};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        attribute_type::AttributeType,
        entity_type_name::EntityTypeName,
        identifier::Identifier,
        scope::path::ScopePath,
    };
    use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
    use andromeda_postgresql_context_store::entity::staged_geometry::StagedGeometry;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
    };
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Arc;

    const VEHICLE: &str = "https://example.org/Vehicle";
    const SPEED: &str = "https://example.org/speed";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn vehicle() -> EntityTypeName {
        VEHICLE.parse().expect("valid type name")
    }

    fn speed() -> AttributeName {
        SPEED.parse().expect("valid attribute name")
    }

    fn location() -> AttributeName {
        LOCATION.parse().expect("valid attribute name")
    }

    /// The keys of `types` and `attributes`.
    fn keys(types: Vec<(EntityTypeName, i32)>, attributes: Vec<(AttributeName, i32)>) -> NameKeys {
        NameKeys::new(
            ResolvedNames::from_pairs(attributes.into_iter().map(|(name, key)| (name, AttributeNameKey::from(key)))),
            ResolvedNames::from_pairs(types.into_iter().map(|(name, key)| (name, EntityTypeNameKey::from(key)))),
        )
    }

    /// The keys of the Vehicle type, `speed` and `location`.
    fn resolved() -> NameKeys {
        keys(vec![(vehicle(), 7)], vec![(speed(), 11), (location(), 13)])
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
        let mut state = EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes");
        state.stamp_attribute_instants(None, SystemInstant::new(Utc::now()));
        state
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
        pretty_assertions::assert_eq!(columns.entities.entity_type_name_keys[0].as_str(), "{7}");
        pretty_assertions::assert_eq!(columns.entities.attribute_name_keys[0].as_str(), "{11}");
        pretty_assertions::assert_eq!(columns.entities.attribute_instance_name_keys[0].as_str(), "{11}");
        pretty_assertions::assert_eq!(
            columns.entities.attribute_instance_kinds[0].as_str(),
            "{1}",
            "a Property is stored as family code 1"
        );
    }

    /// The stored instance arrays are compared whole to decide whether a write
    /// rewrites the search row, so they are encoded in one canonical order
    /// whatever order the state listed its members in.
    #[test]
    fn attribute_instances_encode_in_one_canonical_order() {
        let colour: AttributeName = "https://example.org/colour".parse().expect("valid attribute name");
        let names = keys(vec![(vehicle(), 7)], vec![(speed(), 11), (colour.clone(), 3)]);
        let encode = |attributes: ProjectedAttributes| {
            let mut write = scoped_write("urn:ngsi-ld:A", Vec::new(), None);
            if let EntityMutation::Update(record) = &mut write.entity {
                record.projection.attributes = attributes;
            }
            let writes = [write];
            let indexed: Vec<_> = writes.iter().enumerate().collect();
            let columns = columns_for(&indexed, &names).expect("valid columns");
            (
                columns.entities.attribute_instance_name_keys[0].as_str().to_owned(),
                columns.entities.attribute_instance_kinds[0].as_str().to_owned(),
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
        let names = keys(vec![(vehicle(), 7)], vec![(speed(), 11), (scope.clone(), 5)]);
        let mut write = scoped_write("urn:ngsi-ld:A", Vec::new(), None);
        if let EntityMutation::Update(record) = &mut write.entity {
            record.projection.attributes.push(scope, |_| []);
        }
        let writes = [write];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &names).expect("valid columns");
        pretty_assertions::assert_eq!(columns.entities.attribute_name_keys[0].as_str(), "{11,5}");
        pretty_assertions::assert_eq!(columns.entities.attribute_instance_name_keys[0].as_str(), "{11}");
    }

    /// The keys were minted from these very writes, so a name without one is
    /// a broken invariant rather than a value to default.
    #[test]
    fn a_name_the_dictionary_did_not_resolve_is_an_invariant_failure() {
        let writes = [scoped_write("urn:ngsi-ld:A", Vec::new(), None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let refused = columns_for(&indexed, &NameKeys::default()).expect_err("an unresolved name is refused");
        assert!(matches!(refused, CommitError::Invariant(_)), "{refused:?}");
        let types_only = keys(vec![(vehicle(), 7)], Vec::new());
        let refused = columns_for(&indexed, &types_only).expect_err("an unresolved attribute name is refused");
        assert!(
            matches!(refused, CommitError::Invariant(CommitInvariant::UnresolvedName(NameDictionary::AttributeName))),
            "{refused:?}"
        );
    }

    /// Clause 4.18: the label path is what a `scopeQ` matches, the written
    /// text what reads back.
    #[test]
    fn scopes_are_encoded_as_ltree_paths_beside_their_written_text() {
        let writes = [scoped_write("urn:ngsi-ld:A", vec!["/a/b", "c"], None)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &resolved()).expect("valid columns");
        let scopes = &columns.entities.scopes[0];
        pretty_assertions::assert_eq!(scopes.label_paths().0, ["a.b".to_owned(), "c".to_owned()].as_slice());
        pretty_assertions::assert_eq!(scopes.written().0, ["/a/b".to_owned(), "c".to_owned()].as_slice());
    }

    /// Each attribute instance of each state is one staged row, and a state
    /// without attributes stages none.
    #[test]
    fn each_attribute_instance_is_staged_as_one_row() {
        let writes = [
            scoped_write("urn:ngsi-ld:A", Vec::new(), Some((1.0, 2.0))),
            scoped_write("urn:ngsi-ld:B", Vec::new(), None),
            scoped_write("urn:ngsi-ld:C", Vec::new(), Some((3.0, 4.0))),
        ];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &resolved()).expect("valid columns");
        pretty_assertions::assert_eq!(columns.rows.len(), 2);
    }

    /// The rows hold the state the write settled, so a state whose instances
    /// carry no system times is not written.
    #[test]
    fn an_unstamped_state_is_not_written() {
        let mut write = scoped_write("urn:ngsi-ld:A", Vec::new(), Some((1.0, 2.0)));
        if let EntityMutation::Update(record) = &mut write.entity {
            let mut state = EntityState::clone(&record.state); // clone: the test strips the times of a copy
            state.strip_attribute_instants();
            record.state = Arc::new(state);
        }
        let writes = [write];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let refused = columns_for(&indexed, &resolved()).expect_err("an unstamped state is refused");
        assert!(matches!(refused, CommitError::Persistence(_)), "{refused:?}");
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
            columns.entities.ids.len(),
            columns.entities.entity_type_name_keys.len(),
            columns.entities.attribute_name_keys.len(),
            columns.entities.attribute_instance_name_keys.len(),
            columns.entities.attribute_instance_kinds.len(),
            columns.entities.scopes.len(),
            columns.entities.locations.len(),
            columns.entities.obs_spaces.len(),
            columns.entities.op_spaces.len(),
            columns.entities.location_expires_ats.len(),
            columns.entities.expires_ats.len(),
            columns.entities.min_attribute_expires_ats.len(),
        ] {
            pretty_assertions::assert_eq!(length, 3);
        }
        pretty_assertions::assert_eq!(
            columns.entities.locations.iter().map(|staged| staged.ewkb().is_some()).collect::<Vec<_>>(),
            vec![true, false, true],
            "the location is encoded from the state the record shares"
        );
    }

    /// An update of the state it read, whose record shares that state's
    /// location, replacing it under `precondition`.
    fn update_sharing_the_location(precondition: Precondition) -> AtomicWrite {
        let previous = Arc::new(vehicle_state("urn:ngsi-ld:A", Some((13.4, 52.5))));
        let current = Arc::new(EntityState::clone(&previous)); // clone: shares every attribute, as a read-modify-write does
        AtomicWrite::builder()
            .entity_id(Arc::new("urn:ngsi-ld:A".parse::<Identifier>().expect("valid entity URI")))
            .entity(EntityMutation::Update(EntityRecord::replacing(&previous, current).expect("a projection")))
            .precondition(precondition)
            .build()
    }

    /// The resolution of the Vehicle type and of `location`.
    fn located_names() -> NameKeys {
        keys(vec![(vehicle(), 7)], vec![(location(), 13)])
    }

    /// Clause 5.6.2: an update that leaves `location` as the state it read held
    /// it, and holds against that version, stages the geometry as kept -- no
    /// EWKB is encoded or bound, so the statement never re-projects it.
    #[test]
    fn an_update_holding_against_its_read_keeps_an_unchanged_geometry() {
        let writes = [update_sharing_the_location(Precondition::UnchangedSince(Utc::now()))];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &located_names()).expect("valid columns");
        pretty_assertions::assert_eq!(columns.entities.locations, vec![StagedGeometry::Kept]);
        pretty_assertions::assert_eq!(columns.entities.obs_spaces, vec![StagedGeometry::Kept], "neither state holds one");
    }

    /// An update that overwrites whatever is stored cannot know the stored row
    /// holds the geometry it read, so it writes the geometry.
    #[test]
    fn an_unconditional_update_writes_even_an_unchanged_geometry() {
        let writes = [update_sharing_the_location(Precondition::MustExist)];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &located_names()).expect("valid columns");
        assert!(columns.entities.locations[0].ewkb().is_some(), "{:?}", columns.entities.locations);
        pretty_assertions::assert_eq!(columns.entities.obs_spaces, vec![StagedGeometry::Written(None)]);
    }

    /// An update that states the state it read stages only what differs from
    /// it: restating that state stages no row and no grain.
    #[test]
    fn an_update_of_the_state_it_read_stages_no_row() {
        let writes = [update_sharing_the_location(Precondition::UnchangedSince(Utc::now()))];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &located_names()).expect("valid columns");
        assert!(columns.rows.is_empty(), "{:?}", columns.rows);
        assert!(columns.grains.is_empty(), "{:?}", columns.grains);
    }

    /// An attribute the update removes is staged as a grain to delete, under
    /// its dictionary key, and nothing is written whole.
    #[test]
    fn an_update_removing_an_attribute_stages_its_grain() {
        let previous = Arc::new(vehicle_state("urn:ngsi-ld:A", Some((13.4, 52.5))));
        let current = Arc::new(vehicle_state("urn:ngsi-ld:A", None));
        let writes = [AtomicWrite::builder()
            .entity_id(Arc::new("urn:ngsi-ld:A".parse::<Identifier>().expect("valid entity URI")))
            .entity(EntityMutation::Update(EntityRecord::replacing(&previous, current).expect("a projection")))
            .precondition(Precondition::UnchangedSince(Utc::now()))
            .build()];
        let indexed: Vec<_> = writes.iter().enumerate().collect();
        let columns = columns_for(&indexed, &located_names()).expect("valid columns");
        assert!(columns.rows.is_empty(), "{:?}", columns.rows);
        pretty_assertions::assert_eq!(columns.grains.len(), 1);
        let unresolved = columns_for(&indexed, &keys(vec![(vehicle(), 7)], Vec::new())).expect_err("the removed name has no key");
        assert!(
            matches!(
                unresolved,
                CommitError::Invariant(CommitInvariant::UnresolvedName(NameDictionary::AttributeName))
            ),
            "{unresolved:?}"
        );
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
