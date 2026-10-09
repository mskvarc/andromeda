//! The current-state row an atomic write stores for one entity, in NGSI-LD
//! terms rather than backend columns.

use crate::{commit_invariant::CommitInvariant, error::CommitError, hoisted_geometries::HoistedGeometryChanges, projected_attributes::ProjectedAttributes};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, reads::member_name::MemberName};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// The members of an entity a query filters on, read off its state so the
/// store can index them without walking the stored document.
///
/// Held apart from the state and the expiry instants because it changes for a
/// different reason: an attribute value write replaces the state on every
/// call, while the projection moves only when a type, an attribute name, an
/// attribute instance's family or a scope does. A store that keeps its search
/// indexes and its discovery census on these members alone rewrites them only
/// when this projection changed.
///
/// The hoisted `location`, `observationSpace` and `operationSpace` geometries
/// are not copied in: they are read from the record's state
/// ([`EntityState::location_geometry`] and its siblings), which the record
/// shares, so a store encodes them without a copy of the positions.
#[derive(Debug, Default)]
pub struct SearchProjection {
    /// Entity types from clause 4.5.1, each named once, in declaration order.
    pub types: Vec<EntityTypeName>,
    /// Names of the members clause 5.5.9 `attrs` reads, each with the family
    /// of every instance it holds (clause 4.5.5), which is what clause 5.7
    /// discovery counts. Members NGSI-LD reserves (`scope`, the Entity's
    /// `expiresAt`) are named and hold no instance.
    pub attributes: ProjectedAttributes,
    /// The scope paths the entity belongs to (clause 4.18).
    pub scopes: Vec<ScopePath>,
    /// The clause 4.22 `expiresAt` of the hoisted `location` instance, or
    /// `None` when it carries none.
    pub location_expires_at: Option<DateTime<Utc>>,
}

impl SearchProjection {
    /// Project an entity state in one pass over its members.
    ///
    /// The types are the state's own, each held once (clause 4.16), in the
    /// order the entity declared them: a representation that carries one type
    /// only keeps the first (Table 4.3.6.8-1 note 1).
    ///
    /// # Errors
    ///
    /// Returns [`CommitError::Invariant`] when the expanded name of `scope` or
    /// of the entity's `expiresAt` does not read as an attribute name.
    pub fn from_state(state: &EntityState) -> Result<SearchProjection, CommitError> {
        let mut attributes = ProjectedAttributes::default();
        for member in state.member_names() {
            match member {
                MemberName::Attribute(name) => {
                    let families = state.member_instance_families(name.as_str());
                    attributes.push(name.clone(), |_| families); // clone: the projection owns the names it binds
                }
                MemberName::Scope | MemberName::ExpiresAt => attributes.push(reserved_member_name(member)?, |_| []),
            }
        }
        let attributes = attributes;
        Ok(SearchProjection {
            types: state.types().as_slice().to_vec(), // clone: the store binds the names after the state is shared on
            attributes,
            scopes: state.scope().map(|scopes| scopes.as_slice().to_vec()).unwrap_or_default(), // clone: as the types
            location_expires_at: state.location_expiry(),
        })
    }
}

/// The attribute name a reserved member is projected under.
fn reserved_member_name(member: MemberName<'_>) -> Result<AttributeName, CommitError> {
    member
        .as_str()
        .parse()
        .map_err(|error| CommitError::Invariant(CommitInvariant::ReservedMemberName(error)))
}

/// Entity state written to the current-state store, expressed in NGSI-LD
/// terms rather than backend columns.
#[derive(Debug)]
pub struct EntityRecord {
    /// The entity state, shared with the consequence paths.
    pub state: Arc<EntityState>,
    /// The state the write was computed from and replaces, or `None` when it
    /// replaces no state its writer read. A store that keeps an entity's
    /// attribute instances apart writes only those the two states differ in.
    pub replaces: Option<Arc<EntityState>>,
    /// The members a query filters on.
    pub projection: SearchProjection,
    /// Which hoisted geometries differ from the state this record replaces.
    pub geometry_changes: HoistedGeometryChanges,
    /// Entity `expiresAt` value from clause 4.22, when present.
    pub expires_at: Option<DateTime<Utc>>,
    /// The earliest `expiresAt` any attribute instance carries, when one does
    /// (clause 4.22).
    pub min_attribute_expires_at: Option<DateTime<Utc>>,
}

impl EntityRecord {
    /// Extract the backend-neutral state required to persist an entity that
    /// replaces no state the writer read: every hoisted geometry is changed.
    ///
    /// # Errors
    ///
    /// Returns the [`CommitError::Invariant`] of
    /// [`SearchProjection::from_state`].
    pub fn from_state(state: Arc<EntityState>) -> Result<EntityRecord, CommitError> {
        Ok(EntityRecord {
            projection: SearchProjection::from_state(&state)?,
            geometry_changes: HoistedGeometryChanges::all_changed(),
            expires_at: state.entity_expiry(),
            min_attribute_expires_at: state.min_attribute_expiry(),
            state,
            replaces: None,
        })
    }

    /// Extract the state required to persist `state` in place of `previous`,
    /// the state the write was computed from, recording which hoisted
    /// geometries the write leaves as `previous` held them.
    ///
    /// # Errors
    ///
    /// Returns the [`CommitError::Invariant`] of
    /// [`SearchProjection::from_state`].
    pub fn replacing(previous: &Arc<EntityState>, state: Arc<EntityState>) -> Result<EntityRecord, CommitError> {
        let mut record = EntityRecord::from_state(state)?;
        record.geometry_changes = HoistedGeometryChanges::between(previous, &record.state);
        record.replaces = Some(Arc::clone(previous)); // clone: Arc ref-count bump; the writer keeps the state it read
        Ok(record)
    }

    /// Assemble a record from members a preparation pass already extracted,
    /// so a batch that validated the state does not extract them twice. The
    /// record replaces no state its writer read: every hoisted geometry is
    /// changed.
    #[must_use]
    pub const fn from_parts(
        state: Arc<EntityState>,
        projection: SearchProjection,
        expires_at: Option<DateTime<Utc>>,
        min_attribute_expires_at: Option<DateTime<Utc>>,
    ) -> EntityRecord {
        EntityRecord {
            state,
            replaces: None,
            projection,
            geometry_changes: HoistedGeometryChanges::all_changed(),
            expires_at,
            min_attribute_expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_record::{EntityRecord, SearchProjection},
        hoisted_geometries::GeometryChange,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_geometry::form::GeometryForm;
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, entity_type_name::EntityTypeName};
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// Decode a stored document into a state.
    fn state(document: &Value) -> EntityState {
        EntityState::from_slice(&serde_json::to_vec(document).expect("JSON")).expect("a stored entity decodes")
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("instant").with_timezone(&Utc)
    }

    #[test]
    fn an_entity_record_extracts_semantic_members_from_the_state() {
        let entity = Arc::new(state(&json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/fleet/a"}],
            "https://example.org/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2030-01-01T00:00:00Z"}]
            }]
        }])));
        let record = EntityRecord::from_state(Arc::clone(&entity)).expect("a projection");
        assert!(Arc::ptr_eq(&entity, &record.state));
        pretty_assertions::assert_eq!(record.projection.types.len(), 1);
        pretty_assertions::assert_eq!(record.projection.scopes.len(), 1);
        let members: Vec<(&str, Vec<AttributeType>)> = record
            .projection
            .attributes
            .members()
            .map(|(name, families)| (name.as_str(), families.to_vec()))
            .collect();
        pretty_assertions::assert_eq!(
            members,
            vec![
                ("https://uri.etsi.org/ngsi-ld/scope", Vec::new()),
                ("https://example.org/speed", vec![AttributeType::Property]),
            ],
            "scope is a member of the Entity, named for attrs but holding no attribute instance"
        );
        pretty_assertions::assert_eq!(record.min_attribute_expires_at, Some(instant("2030-01-01T00:00:00Z")));
        assert!(record.expires_at.is_none());
    }

    /// Every `datasetId` instance is one shape, and the family is the one each
    /// instance declares, so one name can carry two families; the Entity's own
    /// `expiresAt` is named and holds no instance.
    #[test]
    fn every_attribute_instance_is_one_shape_with_its_own_family() {
        let entity = state(&json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://example.org/rel": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:B"}]},
                {
                    "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]
                }
            ],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2030-01-01T00:00:00Z"}]
        }]));
        let projection = SearchProjection::from_state(&entity).expect("a projection");
        let members: Vec<(&str, Vec<AttributeType>)> = projection
            .attributes
            .members()
            .map(|(name, families)| (name.as_str(), families.to_vec()))
            .collect();
        pretty_assertions::assert_eq!(
            members,
            vec![
                ("https://uri.etsi.org/ngsi-ld/expiresAt", Vec::new()),
                ("https://example.org/rel", vec![AttributeType::Relationship, AttributeType::Property]),
            ]
        );
        pretty_assertions::assert_eq!(projection.attributes.instance_count(), 2);
        let record = EntityRecord::from_state(Arc::new(entity)).expect("a projection");
        pretty_assertions::assert_eq!(record.expires_at, Some(instant("2030-01-01T00:00:00Z")));
    }

    /// A stored document may repeat a type; the state holds it once, in the
    /// order the entity declared its types, and so does the projection.
    #[test]
    fn a_repeated_type_is_projected_once_in_declaration_order() {
        let entity = state(&json!([{"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/B", "https://example.org/A", "https://example.org/B"]}]));
        let projection = SearchProjection::from_state(&entity).expect("a projection");
        let names: Vec<&str> = projection.types.iter().map(EntityTypeName::as_str).collect();
        pretty_assertions::assert_eq!(names, vec!["https://example.org/B", "https://example.org/A"]);
    }

    /// The geometries a store hoists and the `location` expiry are the default
    /// instances' (clause 4.5.16.1).
    #[test]
    fn the_default_geometries_and_the_location_expiry_are_the_default_instances() {
        let point = |longitude: f64, latitude: f64| {
            json!({
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]
            })
        };
        let entity = state(&json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/location": [
                {
                    "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                    "https://uri.etsi.org/ngsi-ld/hasValue": [point(1.0, 1.0)],
                    "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]
                },
                {
                    "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                    "https://uri.etsi.org/ngsi-ld/hasValue": [point(13.4, 52.5)],
                    "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2031-01-01T00:00:00Z"}]
                }
            ],
            "https://uri.etsi.org/ngsi-ld/operationSpace": [{"@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"], "https://uri.etsi.org/ngsi-ld/hasValue": [point(2.0, 3.0)]}]
        }]));
        let projection = SearchProjection::from_state(&entity).expect("a projection");
        let longitude = |form: Option<&GeometryForm>| match form {
            Some(GeometryForm::Point(position)) => Some(position.longitude()),
            Some(
                GeometryForm::MultiPoint(_)
                | GeometryForm::LineString(_)
                | GeometryForm::MultiLineString(_)
                | GeometryForm::Polygon(_)
                | GeometryForm::MultiPolygon(_),
            )
            | None => None,
        };
        pretty_assertions::assert_eq!(longitude(entity.location_geometry()), Some(13.4));
        pretty_assertions::assert_eq!(longitude(entity.operation_space_geometry()), Some(2.0));
        assert!(entity.observation_space_geometry().is_none());
        pretty_assertions::assert_eq!(projection.location_expires_at, Some(instant("2031-01-01T00:00:00Z")));
    }

    #[test]
    fn a_record_from_parts_keeps_what_the_preparation_pass_extracted() {
        let entity = Arc::new(state(&json!([{"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/A"]}])));
        let record = EntityRecord::from_parts(Arc::clone(&entity), SearchProjection::default(), None, None);
        assert!(Arc::ptr_eq(&entity, &record.state));
        assert_eq!(record.projection.types, [] as [EntityTypeName; 0]);
        assert_eq!(record.geometry_changes.location, GeometryChange::Changed);
    }

    /// A record replacing the state it was computed from records the geometry
    /// the two share as unchanged; a record of the state alone replaces
    /// nothing it read, so its geometry is changed.
    #[test]
    fn a_record_replacing_a_state_records_the_geometries_it_shares() {
        let previous = Arc::new(state(&json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 13.4}, {"@value": 52.5}]}]
                }]
            }]
        }])));
        let current = Arc::new(EntityState::clone(&previous)); // clone: shares every attribute, as a read-modify-write does
        let replacing = EntityRecord::replacing(&previous, Arc::clone(&current)).expect("a projection");
        assert_eq!(replacing.geometry_changes.location, GeometryChange::Unchanged);
        assert!(Arc::ptr_eq(&replacing.state, &current));
        assert!(replacing.replaces.as_ref().is_some_and(|replaced| Arc::ptr_eq(replaced, &previous)));
        let fresh = EntityRecord::from_state(current).expect("a projection");
        assert_eq!(fresh.geometry_changes.location, GeometryChange::Changed);
        assert!(fresh.replaces.is_none());
    }
}
