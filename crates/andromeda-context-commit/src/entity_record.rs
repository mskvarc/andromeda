//! The current-state row an atomic write stores for one entity, in NGSI-LD
//! terms rather than backend columns.

use crate::{error::CommitError, projected_attributes::ProjectedAttributes};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, scope::path::ScopePath};
use andromeda_ngsi_ld_model::expanded::{
    attribute_expiry::min_attribute_expires_at,
    entity_member::{extract_attribute_names, extract_expires_at, extract_types},
    geo_property::{extract_location_expiry, extract_location_geometry, extract_observation_space_geometry, extract_operation_space_geometry},
    instance_key::member_instance_families,
    scope_member::extract_scopes,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::Arc;

/// The members of an entity a query filters on, extracted from its expanded
/// payload so the store can index them without walking the payload.
///
/// Held apart from the payload and the expiry instants because it changes for
/// a different reason: an attribute value write replaces the payload on every
/// call, while the projection moves only when a type, an attribute name, an
/// attribute instance's family, a scope or a geometry does. A store that keeps
/// its search indexes and its discovery census on these members alone rewrites
/// them only when this projection changed.
#[derive(Debug, Default)]
pub struct SearchProjection {
    /// Entity types from clause 4.5.1, each named once.
    pub types: Vec<EntityTypeName>,
    /// Names of the attributes the payload carries (clause 5.5.9 `attrs`),
    /// each with the family of every instance it holds (clause 4.5.5), which
    /// is what clause 5.7 discovery counts. Members NGSI-LD reserves (`scope`,
    /// the Entity's `expiresAt`) are named and hold no instance.
    pub attributes: ProjectedAttributes,
    /// The scope paths the entity belongs to (clause 4.18). A deleted scope
    /// places the entity nowhere, so it contributes no path here.
    pub scopes: Vec<ScopePath>,
    /// Hoisted `location` geometry, when present.
    pub location: Option<Geometry>,
    /// Hoisted `observationSpace` geometry, when present.
    pub observation_space: Option<Geometry>,
    /// Hoisted `operationSpace` geometry, when present.
    pub operation_space: Option<Geometry>,
    /// The clause 4.22 `expiresAt` of the hoisted `location` instance, the one
    /// `location` holds, or `None` when it carries none.
    pub location_expires_at: Option<DateTime<Utc>>,
}

impl SearchProjection {
    /// Extract the projection of an expanded entity payload.
    ///
    /// # Errors
    ///
    /// Returns [`CommitError::Invariant`] when a type or an attribute name of the
    /// payload cannot be read: every write path
    /// admits a payload's members before it builds a record, so an unreadable
    /// one is a broker fault rather than something to index under less than
    /// the payload says.
    pub fn from_expanded(payload: &Value) -> Result<SearchProjection, CommitError> {
        // JSON-LD expansion keeps a type the document repeats, but an Entity
        // carries each of its types once (clause 4.16), and every relation keyed
        // by type — the memberships and the discovery census — counts it once.
        // The first occurrence keeps its place: the declaration order is what a
        // representation that carries one type only keeps the first of (Table
        // 4.3.6.8-1 note 1).
        let declared = extract_types(payload).map_err(|error| CommitError::Invariant(error.into()))?;
        let mut types: Vec<EntityTypeName> = Vec::with_capacity(declared.len());
        for entity_type in declared {
            if !types.contains(&entity_type) {
                types.push(entity_type);
            }
        }
        let types = types;
        let mut attributes = ProjectedAttributes::default();
        for name in extract_attribute_names(payload).map_err(|error| CommitError::Invariant(error.into()))? {
            attributes.push(name, |name| member_instance_families(payload, name));
        }
        let attributes = attributes;
        Ok(SearchProjection {
            types,
            attributes,
            scopes: extract_scopes(payload),
            location: extract_location_geometry(payload),
            observation_space: extract_observation_space_geometry(payload),
            operation_space: extract_operation_space_geometry(payload),
            location_expires_at: extract_location_expiry(payload),
        })
    }
}

/// Entity state written to the current-state store, expressed in NGSI-LD
/// terms rather than backend columns.
#[derive(Debug)]
pub struct EntityRecord {
    /// Expanded JSON-LD payload shared with consequence paths.
    pub payload: Arc<Value>,
    /// The members a query filters on.
    pub projection: SearchProjection,
    /// Entity `expiresAt` value from clause 4.22, when present.
    pub expires_at: Option<DateTime<Utc>>,
    /// The earliest `expiresAt` any attribute instance carries, when one does
    /// (clause 4.22).
    pub min_attribute_expires_at: Option<DateTime<Utc>>,
}

impl EntityRecord {
    /// Extract the backend-neutral state required to persist an expanded
    /// entity payload.
    ///
    /// # Errors
    ///
    /// Returns the [`CommitError::Invariant`] of
    /// [`SearchProjection::from_expanded`].
    pub fn from_expanded(payload: Arc<Value>) -> Result<EntityRecord, CommitError> {
        Ok(EntityRecord {
            projection: SearchProjection::from_expanded(&payload)?,
            expires_at: extract_expires_at(&payload),
            min_attribute_expires_at: min_attribute_expires_at(&payload),
            payload,
        })
    }

    /// Assemble a record from members a preparation pass already extracted,
    /// so a batch that validated the payload does not extract them twice.
    #[must_use]
    pub const fn from_parts(
        payload: Arc<Value>,
        projection: SearchProjection,
        expires_at: Option<DateTime<Utc>>,
        min_attribute_expires_at: Option<DateTime<Utc>>,
    ) -> EntityRecord {
        EntityRecord {
            payload,
            projection,
            expires_at,
            min_attribute_expires_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_record::{EntityRecord, SearchProjection};
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, entity_type_name::EntityTypeName};
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn an_entity_record_extracts_semantic_members_from_expanded_state() {
        let payload = Arc::new(json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/fleet/a"}],
            "https://example.org/speed": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": "2030-01-01T00:00:00Z"}]
            }]
        }]));
        let record = EntityRecord::from_expanded(Arc::clone(&payload)).expect("readable members");
        assert!(Arc::ptr_eq(&payload, &record.payload));
        pretty_assertions::assert_eq!(record.projection.types.len(), 1);
        pretty_assertions::assert_eq!(record.projection.scopes.len(), 1);
        pretty_assertions::assert_eq!(record.projection.attributes.names().len(), 2);
        let members: Vec<(&str, Vec<AttributeType>)> = record
            .projection
            .attributes
            .members()
            .map(|(name, families)| (name.as_str(), families.to_vec()))
            .collect();
        assert!(members.contains(&("https://example.org/speed", vec![AttributeType::Property])), "{members:?}");
        assert!(
            members.contains(&("https://uri.etsi.org/ngsi-ld/scope", Vec::new())),
            "scope is a member of the Entity, named for attrs but holding no attribute instance: {members:?}"
        );
        pretty_assertions::assert_eq!(
            record.min_attribute_expires_at,
            Some(DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z").expect("instant").with_timezone(&Utc))
        );
        assert!(record.expires_at.is_none());
    }

    /// Every `datasetId` instance is one shape, and the family is the one each
    /// instance declares, so one name can carry two families.
    #[test]
    fn every_attribute_instance_is_one_shape_with_its_own_family() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://example.org/rel": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}]}
            ],
            "https://uri.etsi.org/ngsi-ld/expiresAt": [{"@value": "2030-01-01T00:00:00Z"}]
        }]);
        let projection = SearchProjection::from_expanded(&payload).expect("readable members");
        let members: Vec<(&str, Vec<AttributeType>)> = projection
            .attributes
            .members()
            .map(|(name, families)| (name.as_str(), families.to_vec()))
            .filter(|(_, families)| !families.is_empty())
            .collect();
        pretty_assertions::assert_eq!(
            members,
            vec![("https://example.org/rel", vec![AttributeType::Relationship, AttributeType::Property])],
            "the Entity's own expiresAt holds no attribute instance"
        );
        pretty_assertions::assert_eq!(projection.attributes.instance_count(), 2);
    }

    /// JSON-LD expansion keeps a repeated type, the projection names it once,
    /// in the order the entity declared its types.
    #[test]
    fn a_repeated_type_is_projected_once_in_declaration_order() {
        let payload = json!([{"@id": "urn:ngsi-ld:A", "@type": ["https://example.org/B", "https://example.org/A", "https://example.org/B"]}]);
        let projection = SearchProjection::from_expanded(&payload).expect("readable members");
        let names: Vec<&str> = projection.types.iter().map(EntityTypeName::as_str).collect();
        pretty_assertions::assert_eq!(names, vec!["https://example.org/B", "https://example.org/A"]);
    }

    #[test]
    fn a_record_from_parts_keeps_what_the_preparation_pass_extracted() {
        let payload = Arc::new(json!([{"@id": "urn:ngsi-ld:A"}]));
        let record = EntityRecord::from_parts(Arc::clone(&payload), SearchProjection::default(), None, None);
        assert!(Arc::ptr_eq(&payload, &record.payload));
        assert_eq!(record.projection.types, [] as [EntityTypeName; 0]);
    }
}
