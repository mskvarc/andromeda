//! The EntityMap resource that pins a distributed query's result set (ETSI GS CIM 009 clause 5.2.39).

use andromeda_ngsi_ld_lexicon::{constant_string, ngsi_ld_urn::EntityMapId, uri::Uri};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use compact_str::CompactString;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

constant_string!(EntityMapType, "EntityMap");

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
    ///
    /// The values are clause 4.5.5 `datasetId`s, the same type a temporal
    /// query filters on: `@none` names the default instance and anything else
    /// is a URN.
    #[builder(default)]
    pub entity_map: IndexMap<Uri, Vec<DatasetSelector>>,
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
            entity_map.insert(entity_id.clone(), vec![DatasetSelector::Default]);
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
    fn a_new_entity_map_records_the_default_instance_of_every_entity() {
        // Clause 4.5.5: an attribute instance with no `datasetId` is the
        // default one, which the map spells `@none`.
        let id: EntityMapId = "urn:ngsi-ld:EntityMap:1".parse().expect("valid entity map id");
        let entity: Uri = "urn:ngsi-ld:Vehicle:1".parse().expect("valid entity id");
        let map = EntityMap::new(id, Utc::now(), vec![entity.clone()]);
        pretty_assertions::assert_eq!(map.entity_map.get(&entity), Some(&vec![DatasetSelector::Default]));
    }

    #[test]
    fn an_entity_map_dataset_id_round_trips_through_its_wire_form() {
        let map: IndexMap<Uri, Vec<DatasetSelector>> = serde_json::from_value(serde_json::json!({
            "urn:ngsi-ld:Vehicle:1": ["@none", "urn:ngsi-ld:Dataset:A"]
        }))
        .expect("deserialises");
        let entity: Uri = "urn:ngsi-ld:Vehicle:1".parse().expect("valid entity id");
        pretty_assertions::assert_eq!(
            map.get(&entity),
            Some(&vec![
                DatasetSelector::Default,
                DatasetSelector::Id("urn:ngsi-ld:Dataset:A".parse().expect("valid urn")),
            ])
        );
        pretty_assertions::assert_eq!(
            serde_json::to_value(&map).expect("serialises"),
            serde_json::json!({ "urn:ngsi-ld:Vehicle:1": ["@none", "urn:ngsi-ld:Dataset:A"] })
        );
    }
}
