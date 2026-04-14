use crate::domain::{ngsi_ld_urn::EntityMapId, uri::Uri};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use urn::Urn;

/// JSON-LD `@type` for EntityMap resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityMapType {
    #[serde(rename = "EntityMap")]
    EntityMap,
}

/// Dataset identifier reference inside `entityMap` values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum EntityMapDatasetId {
    /// Default dataset marker.
    None,
    /// Concrete dataset identifier URI.
    Uri(Urn),
}

impl From<EntityMapDatasetId> for String {
    fn from(value: EntityMapDatasetId) -> String {
        match value {
            EntityMapDatasetId::None => "@none".to_string(),
            EntityMapDatasetId::Uri(uri) => uri.to_string(),
        }
    }
}

impl TryFrom<String> for EntityMapDatasetId {
    type Error = EntityMapDatasetIdParseError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value == "@none" {
            return Ok(EntityMapDatasetId::None);
        }
        let uri = value.parse::<Urn>().map_err(|_| EntityMapDatasetIdParseError::InvalidUri)?;
        Ok(EntityMapDatasetId::Uri(uri))
    }
}

/// Parsing errors for [`EntityMapDatasetId`].
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum EntityMapDatasetIdParseError {
    #[error("entityMap dataset id must be '@none' or a valid URI")]
    InvalidUri,
}

/// NGSI-LD EntityMap representation (ETSI GS CIM 009 clause 5.2.39).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityMap {
    pub id: EntityMapId,
    #[serde(rename = "type")]
    pub type_: EntityMapType,
    pub expires_at: DateTime<Utc>,
    #[serde(rename = "ids")]
    pub entity_ids: Vec<Uri>,
    pub entity_map: IndexMap<Uri, Vec<EntityMapDatasetId>>,
    pub linked_maps: IndexMap<CompactString, EntityMapId>,
}

impl EntityMap {
    /// Construct a new EntityMap with default `@none` dataset mapping per entity.
    pub fn new(id: EntityMapId, expires_at: DateTime<Utc>, entity_ids: Vec<Uri>) -> EntityMap {
        let mut entity_map = IndexMap::new();
        for entity_id in &entity_ids {
            entity_map.insert(entity_id.clone(), vec![EntityMapDatasetId::None]);
        }
        EntityMap {
            id,
            type_: EntityMapType::EntityMap,
            expires_at,
            entity_ids,
            entity_map,
            linked_maps: IndexMap::new(),
        }
    }

    /// Returns whether this map has already expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}
