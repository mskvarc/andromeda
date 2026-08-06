//! HTTP request model types for entity map query endpoints.

use crate::query::{entity::EntityQueryParameters, entity_selector::EntitySelectorParams};
use serde::Deserialize;
use serde_json::Value;

/// Request body for POST-based entity map creation (section 5.14.4, 6.34.3.2).
#[derive(Debug, Deserialize)]
pub struct QueryRequestBody {
    /// Entity selectors constraining the query.
    pub entities: Option<Vec<EntitySelector>>,
    /// Attribute names to include.
    pub attrs: Option<Vec<String>>,
    /// NGSI-LD query expression.
    pub q: Option<String>,
    /// Geo-relationship for geo-queries.
    pub georel: Option<String>,
    /// Geometry type for geo-queries.
    pub geometry: Option<String>,
    /// Coordinates for geo-queries.
    pub coordinates: Option<String>,
    /// Geo-property for geo-queries.
    pub geoproperty: Option<String>,
    /// Scope query expression.
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<String>,
    /// Requested entity map lifetime as ISO 8601 duration.
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
}

/// A single entity selector within a query request body.
#[derive(Debug, Deserialize)]
pub struct EntitySelector {
    /// Entity IDs to match.
    pub id: Option<Vec<String>>,
    /// ID pattern to match.
    #[serde(rename = "idPattern")]
    pub id_pattern: Option<String>,
    /// Entity type to match.
    #[serde(rename = "type")]
    pub type_: Option<String>,
}

/// Request body for PATCH entity map (section 5.14.2).
#[derive(Debug, Deserialize)]
pub struct UpdateEntityMapBody {
    /// New expiration time.
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<String>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[serde(rename = "entityMap")]
    #[allow(dead_code)]
    pub entity_map: Option<Value>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[serde(rename = "linkedMaps")]
    #[allow(dead_code)]
    pub linked_maps: Option<Value>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub type_: Option<String>,
    /// Output-only field -- accepted but ignored (section 5.14.2.4).
    #[allow(dead_code)]
    pub id: Option<String>,
}

/// Convert a `QueryRequestBody` into [`EntityQueryParameters`].
///
/// The POST body form of a query is defined by conversion into the GET
/// parameter form, so this maps each body field onto the matching parameter
/// directly. It does not round-trip through a URL-encoded string: that path
/// discarded the client's whole selection and substituted an unfiltered
/// `local=true` query on any encode/parse hiccup, which is a silently wrong
/// answer rather than a rejected request. Every selection member the body
/// carries -- including `scopeQ` and `entityMapLifetime`, which the round-trip
/// dropped -- is preserved. Entity-map creation is always confined to this
/// broker, so `local` is set.
pub fn query_body_to_params(body: &QueryRequestBody) -> EntityQueryParameters {
    let (id, entity_type, id_pattern) = if let Some(entities) = &body.entities {
        let ids: Vec<&str> = entities.iter().flat_map(|e| e.id.iter().flatten()).map(String::as_str).collect();
        let types: Vec<&str> = entities.iter().filter_map(|e| e.type_.as_deref()).collect();
        (
            if ids.is_empty() { None } else { Some(ids.join(",")) },
            if types.is_empty() { None } else { Some(types.join(",")) },
            entities.iter().find_map(|e| e.id_pattern.clone()),
        )
    } else {
        (None, None, None)
    };

    // The precise per-selector grouping (clause 5.2.11), kept alongside the
    // flattened summary above so `{id:A,type:T1}` and `{id:B,type:T2}` are a
    // union rather than the cross product the summary would express.
    let entity_selectors = body
        .entities
        .as_ref()
        .map(|entities| {
            entities
                .iter()
                .map(|selector| EntitySelectorParams {
                    id: selector.id.as_ref().map(|ids| ids.join(",")),
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
        attrs: body.attrs.as_ref().map(|a| a.join(",")),
        q: body.q.clone(),
        georel: body.georel.clone(),
        geometry: body.geometry.clone(),
        coordinates: body.coordinates.clone(),
        geoproperty: body.geoproperty.clone(),
        limit: None,
        offset: None,
        count: None,
        options: None,
        // Entity-map creation is always confined to this broker (clause 5.14).
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
        entity_map_lifetime: body.entity_map_lifetime.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_body_maps_every_selection_member_without_a_lossy_fallback() {
        // The POST body form of a query maps field for field onto the GET
        // parameter form. A body that carries filters must keep them: the old
        // URL round-trip dropped `entityMapLifetime` entirely and replaced the
        // whole selection with an unfiltered `local=true` query on any encode or
        // parse hiccup.
        let body = QueryRequestBody {
            entities: Some(vec![EntitySelector {
                id: Some(vec!["urn:ngsi-ld:Vehicle:1".to_string()]),
                id_pattern: None,
                type_: Some("Vehicle".to_string()),
            }]),
            attrs: Some(vec!["speed".to_string(), "brandName".to_string()]),
            q: Some("speed>50".to_string()),
            georel: Some("near;maxDistance==2000".to_string()),
            geometry: Some("Point".to_string()),
            coordinates: Some("[13.4,52.5]".to_string()),
            geoproperty: Some("location".to_string()),
            scope_q: Some("/Madrid/Gardens".to_string()),
            entity_map_lifetime: Some("PT1H".to_string()),
        };
        let params = query_body_to_params(&body);
        pretty_assertions::assert_eq!(params.id.as_deref(), Some("urn:ngsi-ld:Vehicle:1"));
        pretty_assertions::assert_eq!(params.entity_type.as_deref(), Some("Vehicle"));
        pretty_assertions::assert_eq!(params.attrs.as_deref(), Some("speed,brandName"));
        pretty_assertions::assert_eq!(params.q.as_deref(), Some("speed>50"));
        pretty_assertions::assert_eq!(params.georel.as_deref(), Some("near;maxDistance==2000"));
        pretty_assertions::assert_eq!(params.geometry.as_deref(), Some("Point"));
        pretty_assertions::assert_eq!(params.coordinates.as_deref(), Some("[13.4,52.5]"));
        pretty_assertions::assert_eq!(params.geoproperty.as_deref(), Some("location"));
        pretty_assertions::assert_eq!(params.scope_q.as_deref(), Some("/Madrid/Gardens"));
        pretty_assertions::assert_eq!(params.entity_map_lifetime.as_deref(), Some("PT1H"));
        pretty_assertions::assert_eq!(params.local, Some(true));
    }

    #[test]
    fn query_body_with_no_entities_still_localises() {
        let body = QueryRequestBody {
            entities: None,
            attrs: None,
            q: Some("speed>50".to_string()),
            georel: None,
            geometry: None,
            coordinates: None,
            geoproperty: None,
            scope_q: None,
            entity_map_lifetime: None,
        };
        let params = query_body_to_params(&body);
        assert!(params.id.is_none());
        assert!(params.entity_type.is_none());
        pretty_assertions::assert_eq!(params.q.as_deref(), Some("speed>50"));
        pretty_assertions::assert_eq!(params.local, Some(true));
    }
}
