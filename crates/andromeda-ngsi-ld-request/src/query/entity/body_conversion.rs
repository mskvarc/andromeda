//! Conversion from the entity query POST body to parameter form.

use super::parameters::EntityQueryParameters;
use crate::{
    body::{
        entity_map::{EntitySelector, QueryRequestBody},
        query::QueryRequest,
    },
    query::{deserialization::OptionalGeoQuery, entity_selector::EntitySelectorParams},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;

impl From<QueryRequest> for EntityQueryParameters {
    fn from(request: QueryRequest) -> EntityQueryParameters {
        let mut ids = Vec::new();
        let mut id_pattern = None;
        let mut entity_selectors = Vec::new();

        if let Some(entities) = &request.entities {
            for selection in entities {
                if let Some(id) = selection.id() {
                    ids.push(id.clone());
                }
                if let Some(pattern) = selection.id_pattern() {
                    id_pattern = Some(pattern.clone());
                }
                entity_selectors.push(EntitySelectorParams {
                    id: selection.id().map(|id| vec![id.clone()]),
                    id_pattern: selection.id_pattern().cloned(),
                    type_: selection.type_().cloned(),
                });
            }
        }

        EntityQueryParameters {
            id: if ids.is_empty() { None } else { Some(ids) },
            entity_type: None,
            id_pattern,
            entity_selectors,
            attrs: request.attrs,
            q: request.q,
            geo: OptionalGeoQuery(request.geo_q.map(|geo| geo.0)),
            limit: request.limit,
            offset: request.offset,
            count: request.count,
            options: None,
            local: None,
            pick: None,
            omit: None,
            order_by: None,
            scope_q: None,
            lang: None,
            dataset_id: None,
            expand_values: None,
            json_keys: None,
            format: None,
            join: None,
            join_level: None,
            contained_by: None,
            entity_map: None,
            entity_map_lifetime: None,
            split_entities: None,
            geometry_property: None,
            collation: None,
            order_from: None,
            order_geometry: None,
            csf: None,
            drop: None,
            keep: None,
        }
    }
}

/// Convert an entity-map POST body into entity-query parameters without a
/// lossy URL round trip.
pub fn query_body_to_params(body: &QueryRequestBody) -> EntityQueryParameters {
    let (id, entity_type, id_pattern) = if let Some(entities) = &body.entities {
        let ids: Vec<Uri> = entities.iter().flat_map(|entity| entity.id.iter().flatten().cloned()).collect();
        (
            if ids.is_empty() { None } else { Some(ids) },
            None,
            entities.iter().find_map(|entity| entity.id_pattern.clone()),
        )
    } else {
        (None, None, None)
    };

    let entity_selectors = body
        .entities
        .as_ref()
        .map(|entities| {
            entities
                .iter()
                .map(|selector: &EntitySelector| EntitySelectorParams {
                    id: selector.id.clone(),
                    id_pattern: selector.id_pattern.clone(),
                    type_: selector.type_.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    EntityQueryParameters {
        id,
        entity_type,
        id_pattern,
        entity_selectors,
        attrs: body.attrs.clone(),
        q: body.q.clone(),
        geo: body.geo.clone(),
        limit: None,
        offset: None,
        count: None,
        options: None,
        local: Some(true),
        pick: None,
        omit: None,
        order_by: None,
        scope_q: body.scope_q.clone(),
        lang: None,
        dataset_id: None,
        expand_values: None,
        json_keys: None,
        format: None,
        join: None,
        join_level: None,
        contained_by: None,
        entity_map: None,
        entity_map_lifetime: body.entity_map_lifetime,
        split_entities: None,
        geometry_property: None,
        collation: None,
        order_from: None,
        order_geometry: None,
        csf: None,
        drop: None,
        keep: None,
    }
}
