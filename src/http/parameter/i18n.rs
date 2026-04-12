use crate::http::parameter::{deserialize_option_bool_case_insensitive, entity::GetEntityParams, query::entity::EntityQueryParameters};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedEntityGetParams {
    pub attrs: Option<String>,
    pub options: Option<String>,
    pub format: Option<String>,
    pub pick: Option<String>,
    pub omit: Option<String>,
    pub lang: Option<String>,
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
    pub join: Option<String>,
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    #[serde(rename = "containedBy")]
    pub contained_by: Option<String>,
    #[serde(rename = "entityMap")]
    pub entity_map: Option<String>,
    pub locale: String,
    pub calendar: Option<String>,
    #[serde(rename = "numberingSystem")]
    pub numbering_system: Option<String>,
    #[serde(rename = "localizedAttrs")]
    pub localized_attrs: Option<String>,
}

impl LocalizedEntityGetParams {
    pub fn into_get_params(self) -> GetEntityParams {
        GetEntityParams {
            attrs: self.attrs,
            options: self.options,
            format: self.format,
            pick: self.pick,
            omit: self.omit,
            lang: self.lang,
            dataset_id: self.dataset_id,
            geometry_property: self.geometry_property,
            local: self.local,
            join: self.join,
            join_level: self.join_level,
            contained_by: self.contained_by,
            entity_map: self.entity_map,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedEntityQueryParams {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    pub attrs: Option<String>,
    pub q: Option<String>,
    pub georel: Option<String>,
    pub geometry: Option<String>,
    pub coordinates: Option<String>,
    pub geoproperty: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub count: Option<bool>,
    pub options: Option<String>,
    #[serde(rename = "local")]
    pub local: Option<bool>,
    pub pick: Option<String>,
    pub omit: Option<String>,
    #[serde(rename = "orderBy")]
    pub order_by: Option<String>,
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<String>,
    pub lang: Option<String>,
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(rename = "expandValues")]
    pub expand_values: Option<String>,
    #[serde(rename = "jsonKeys")]
    pub json_keys: Option<String>,
    pub format: Option<String>,
    pub join: Option<String>,
    #[serde(rename = "joinLevel")]
    pub join_level: Option<u32>,
    #[serde(rename = "containedBy")]
    pub contained_by: Option<String>,
    #[serde(rename = "entityMap")]
    pub entity_map: Option<bool>,
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
    #[serde(rename = "splitEntities")]
    pub split_entities: Option<bool>,
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<String>,
    pub collation: Option<String>,
    #[serde(rename = "orderFrom")]
    pub order_from: Option<String>,
    #[serde(rename = "orderGeometry")]
    pub order_geometry: Option<String>,
    pub collate: Option<String>,
    pub locale: String,
    pub calendar: Option<String>,
    #[serde(rename = "numberingSystem")]
    pub numbering_system: Option<String>,
    #[serde(rename = "localizedAttrs")]
    pub localized_attrs: Option<String>,
}

impl LocalizedEntityQueryParams {
    pub fn into_entity_query(self) -> EntityQueryParameters {
        EntityQueryParameters {
            id: self.id,
            entity_type: self.entity_type,
            id_pattern: self.id_pattern,
            attrs: self.attrs,
            q: self.q,
            georel: self.georel,
            geometry: self.geometry,
            coordinates: self.coordinates,
            geoproperty: self.geoproperty,
            limit: self.limit,
            offset: self.offset,
            count: self.count,
            options: self.options,
            local: self.local,
            pick: self.pick,
            omit: self.omit,
            order_by: self.order_by,
            scope_q: self.scope_q,
            lang: self.lang,
            dataset_id: self.dataset_id,
            expand_values: self.expand_values,
            json_keys: self.json_keys,
            format: self.format,
            join: self.join,
            join_level: self.join_level,
            contained_by: self.contained_by,
            entity_map: self.entity_map,
            entity_map_lifetime: self.entity_map_lifetime,
            split_entities: self.split_entities,
            geometry_property: self.geometry_property,
            collation: self.collation,
            order_from: self.order_from,
            order_geometry: self.order_geometry,
            collate: self.collate,
        }
    }
}
