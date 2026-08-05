//! The EntityMap resource that pins a distributed query's result set (ETSI GS CIM 009 clause 5.2.39).

use crate::{constant_string, ngsi_ld_urn::EntityMapId, uri::Uri};
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use derive_more::Display;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;
use typed_builder::TypedBuilder;
use urn_rs::Urn;

constant_string!(EntityMapType, "EntityMap");

/// Dataset identifier reference inside `entityMap` values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display)]
#[serde(try_from = "String", into = "String")]
pub enum EntityMapDatasetId {
    /// Default dataset marker.
    #[display("@none")]
    None,
    /// Concrete dataset identifier URI.
    #[display("{_0}")]
    Uri(Urn),
}

// The wire form is produced by the derived `Display`; this exists only because
// `#[serde(into = "String")]` demands an `Into<String>` and cannot use `Display`.
impl From<EntityMapDatasetId> for String {
    fn from(value: EntityMapDatasetId) -> String {
        value.to_string()
    }
}

// Validating parser: `@none` is a token clause 4.5.5 fixes, everything else
// must be a URN. No derive expresses a parse that branches on a sentinel.
impl FromStr for EntityMapDatasetId {
    type Err = EntityMapDatasetIdParseError;

    fn from_str(value: &str) -> Result<EntityMapDatasetId, EntityMapDatasetIdParseError> {
        if value == "@none" {
            return Ok(EntityMapDatasetId::None);
        }
        let uri = value.parse::<Urn>().map_err(|_| EntityMapDatasetIdParseError::InvalidUri)?;
        Ok(EntityMapDatasetId::Uri(uri))
    }
}

// Parsing lives in `FromStr` so borrowed call sites do not allocate; this
// adapter exists only because `#[serde(try_from = "String")]` requires it, and
// it consumes the `String` serde has already built.
impl TryFrom<String> for EntityMapDatasetId {
    type Error = EntityMapDatasetIdParseError;

    fn try_from(value: String) -> Result<EntityMapDatasetId, EntityMapDatasetIdParseError> {
        value.parse()
    }
}

/// Parsing errors for [`EntityMapDatasetId`].
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum EntityMapDatasetIdParseError {
    /// The value was neither `@none` nor a valid URI.
    #[error("entityMap dataset id must be '@none' or a valid URI")]
    InvalidUri,
}

/// NGSI-LD EntityMap representation (ETSI GS CIM 009 clause 5.2.39).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct EntityMap {
    /// Generated identifier of this map.
    pub id: EntityMapId,
    /// Always `EntityMap`.
    #[serde(rename = "type")]
    #[builder(default)]
    pub type_: EntityMapType,
    /// When the map stops being usable for follow-up pages.
    pub expires_at: DateTime<Utc>,
    /// Identifiers of every entity the originating query matched, in order.
    #[serde(rename = "ids")]
    #[builder(default)]
    pub entity_ids: Vec<Uri>,
    /// Datasets held for each matched entity.
    #[builder(default)]
    pub entity_map: IndexMap<Uri, Vec<EntityMapDatasetId>>,
    /// Maps held by context sources that contributed to the same query.
    #[builder(default)]
    pub linked_maps: IndexMap<CompactString, EntityMapId>,
}

impl EntityMap {
    /// Construct a new EntityMap with default `@none` dataset mapping per entity.
    pub fn new(id: EntityMapId, expires_at: DateTime<Utc>, entity_ids: Vec<Uri>) -> EntityMap {
        let mut entity_map = IndexMap::new();
        for entity_id in &entity_ids {
            // clone: the map owns its keys and `entity_ids` keeps the originals
            entity_map.insert(entity_id.clone(), vec![EntityMapDatasetId::None]);
        }
        EntityMap::builder()
            .id(id)
            .expires_at(expires_at)
            .entity_ids(entity_ids)
            .entity_map(entity_map)
            .build()
    }

    /// Returns whether this map has already expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_entity_map_dataset_id_parses_from_a_borrowed_str() {
        pretty_assertions::assert_eq!("@none".parse::<EntityMapDatasetId>().expect("none"), EntityMapDatasetId::None);
        let parsed = "urn:ngsi-ld:Dataset:A".parse::<EntityMapDatasetId>().expect("urn");
        pretty_assertions::assert_eq!(String::from(parsed), "urn:ngsi-ld:Dataset:A");
        assert!("not a urn".parse::<EntityMapDatasetId>().is_err());
    }
}
