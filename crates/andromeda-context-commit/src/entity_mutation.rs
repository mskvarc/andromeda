//! Entity-row mutations carried by atomic context writes.

use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, scope::grammar::ScopeBuf};
use andromeda_ngsi_ld_model::expanded::{
    entity_member::{extract_expires_at, extract_scopes, extract_types},
    geo_property::{extract_location_geometry, extract_observation_space_geometry, extract_operation_space_geometry},
};
use chrono::{DateTime, Utc};
use derive_more::IsVariant;
use serde_json::Value;
use std::sync::Arc;

/// Entity state written to the current-state store, expressed in NGSI-LD
/// terms rather than backend columns.
#[derive(Debug)]
pub struct EntityRecord {
    /// Expanded JSON-LD payload shared with consequence paths.
    pub payload: Arc<Value>,
    /// Entity types from clause 4.5.1.
    pub types: Vec<EntityTypeName>,
    /// Entity scopes from clause 4.18.
    pub scopes: Vec<ScopeBuf>,
    /// Hoisted `location` geometry, when present.
    pub location: Option<Geometry>,
    /// Hoisted `observationSpace` geometry, when present.
    pub observation_space: Option<Geometry>,
    /// Hoisted `operationSpace` geometry, when present.
    pub operation_space: Option<Geometry>,
    /// Entity `expiresAt` value from clause 4.22, when present.
    pub expires_at: Option<DateTime<Utc>>,
}

impl EntityRecord {
    /// Extract the backend-neutral state required to persist an expanded
    /// entity payload.
    pub fn from_expanded(payload: Arc<Value>) -> EntityRecord {
        EntityRecord {
            types: extract_types(&payload),
            scopes: extract_scopes(&payload),
            location: extract_location_geometry(&payload),
            observation_space: extract_observation_space_geometry(&payload),
            operation_space: extract_operation_space_geometry(&payload),
            expires_at: extract_expires_at(&payload),
            payload,
        }
    }
}

/// Change made to the current entity row.
#[derive(Debug, IsVariant)]
pub enum EntityMutation {
    /// Insert a row that is not present.
    Insert(EntityRecord),
    /// Replace an existing row.
    Update(EntityRecord),
    /// Delete an existing row.
    Delete,
    /// Leave current state unchanged while committing other consequences.
    None,
}

impl EntityMutation {
    /// Return the entity state written by an insert or update.
    pub const fn record(&self) -> Option<&EntityRecord> {
        match self {
            EntityMutation::Insert(record) | EntityMutation::Update(record) => Some(record),
            EntityMutation::Delete | EntityMutation::None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_mutation::{EntityMutation, EntityRecord};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn an_entity_record_extracts_semantic_members_from_expanded_state() {
        let payload = Arc::new(json!([{
            "@id": "urn:ngsi-ld:A",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/fleet/a"}]
        }]));
        let record = EntityRecord::from_expanded(Arc::clone(&payload));
        assert!(Arc::ptr_eq(&payload, &record.payload));
        pretty_assertions::assert_eq!(record.types.len(), 1);
        pretty_assertions::assert_eq!(record.scopes.len(), 1);
    }

    #[test]
    fn only_insert_and_update_mutations_carry_entity_records() {
        assert!(EntityMutation::Insert(EntityRecord::from_expanded(Arc::new(json!([{}])))).record().is_some());
        assert!(EntityMutation::Delete.record().is_none());
        assert!(EntityMutation::None.record().is_none());
    }
}
