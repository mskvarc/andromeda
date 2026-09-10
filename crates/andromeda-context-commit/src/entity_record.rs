//! The current-state row an atomic write stores for one entity, in NGSI-LD
//! terms rather than backend columns.

use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, scope::path::ScopePath};
use andromeda_ngsi_ld_model::expanded::{
    attribute_expiry::min_attribute_expires_at,
    entity_member::{extract_attribute_names, extract_expires_at, extract_types},
    geo_property::{extract_location_expiry, extract_location_geometry, extract_observation_space_geometry, extract_operation_space_geometry},
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
/// call, while the projection moves only when a type, an attribute name, a
/// scope or a geometry does. A store that keeps its search indexes on these
/// members alone rewrites them only when this projection changed.
#[derive(Debug, Default)]
pub struct SearchProjection {
    /// Entity types from clause 4.5.1.
    pub types: Vec<EntityTypeName>,
    /// Names of the attributes the payload carries (clause 5.5.9 `attrs`).
    pub attribute_names: Vec<AttributeName>,
    /// The scope paths the entity belongs to (clause 4.18). A deleted scope
    /// places the entity nowhere, so it contributes no path here.
    pub scopes: Vec<ScopePath>,
    /// Hoisted `location` geometry, when present.
    pub location: Option<Geometry>,
    /// Hoisted `observationSpace` geometry, when present.
    pub observation_space: Option<Geometry>,
    /// Hoisted `operationSpace` geometry, when present.
    pub operation_space: Option<Geometry>,
    /// The instant after which every `location` instance has expired, or
    /// `None` while at least one never does (clause 4.22).
    pub location_expires_at: Option<DateTime<Utc>>,
}

impl SearchProjection {
    /// Extract the projection of an expanded entity payload.
    #[must_use]
    pub fn from_expanded(payload: &Value) -> SearchProjection {
        SearchProjection {
            types: extract_types(payload),
            attribute_names: extract_attribute_names(payload),
            scopes: extract_scopes(payload),
            location: extract_location_geometry(payload),
            observation_space: extract_observation_space_geometry(payload),
            operation_space: extract_operation_space_geometry(payload),
            location_expires_at: extract_location_expiry(payload),
        }
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
    #[must_use]
    pub fn from_expanded(payload: Arc<Value>) -> EntityRecord {
        EntityRecord {
            projection: SearchProjection::from_expanded(&payload),
            expires_at: extract_expires_at(&payload),
            min_attribute_expires_at: min_attribute_expires_at(&payload),
            payload,
        }
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
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
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
        let record = EntityRecord::from_expanded(Arc::clone(&payload));
        assert!(Arc::ptr_eq(&payload, &record.payload));
        pretty_assertions::assert_eq!(record.projection.types.len(), 1);
        pretty_assertions::assert_eq!(record.projection.scopes.len(), 1);
        pretty_assertions::assert_eq!(record.projection.attribute_names.len(), 2);
        pretty_assertions::assert_eq!(
            record.min_attribute_expires_at,
            Some(DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z").expect("instant").with_timezone(&Utc))
        );
        assert!(record.expires_at.is_none());
    }

    #[test]
    fn a_record_from_parts_keeps_what_the_preparation_pass_extracted() {
        let payload = Arc::new(json!([{"@id": "urn:ngsi-ld:A"}]));
        let record = EntityRecord::from_parts(Arc::clone(&payload), SearchProjection::default(), None, None);
        assert!(Arc::ptr_eq(&payload, &record.payload));
        assert_eq!(record.projection.types, [] as [EntityTypeName; 0]);
    }
}
