//! HTTP request model types for entity map query endpoints.

use crate::query::{
    deserialization::{OptionalGeoQuery, deserialize_optional_filter_expr},
    entity::EntityQueryParameters,
    entity_selector::EntitySelectorParams,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::{filter::expression::Expr, pattern::Pattern, scope::query::ScopeQuery, type_selection::grammar::TypeExprRaw};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

/// Request body for POST-based entity map creation (section 5.14.4, 6.34.3.2).
#[derive(Debug, Deserialize)]
pub struct QueryRequestBody {
    /// Entity selectors constraining the query.
    pub entities: Option<Vec<EntitySelector>>,
    /// Attribute names to include.
    pub attrs: Option<Vec<AttributeName>>,
    /// NGSI-LD query expression.
    #[serde(default, deserialize_with = "deserialize_optional_filter_expr")]
    pub q: Option<Expr>,
    /// Complete clause-4.10 geo-query parsed at body extraction.
    #[serde(flatten)]
    pub geo: OptionalGeoQuery,
    /// Scope query expression.
    #[serde(rename = "scopeQ")]
    pub scope_q: Option<ScopeQuery>,
    /// Requested entity map lifetime as ISO 8601 duration.
    #[serde(rename = "entityMapLifetime")]
    pub entity_map_lifetime: Option<String>,
}

/// A single entity selector within a query request body.
#[derive(Debug, Deserialize)]
pub struct EntitySelector {
    /// Entity IDs to match.
    pub id: Option<Vec<Uri>>,
    /// ID pattern to match.
    #[serde(rename = "idPattern")]
    #[serde(default, deserialize_with = "crate::query::deserialization::deserialize_optional_pattern")]
    pub id_pattern: Option<Pattern>,
    /// Entity type to match.
    #[serde(rename = "type")]
    pub type_: Option<TypeExprRaw>,
}

/// Request body for PATCH entity map (section 5.14.2).
#[derive(Debug, Deserialize)]
pub struct UpdateEntityMapBody {
    /// New expiration time.
    #[serde(rename = "expiresAt")]
    pub expires_at: Option<DateTime<Utc>>,
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
        let ids: Vec<Uri> = entities.iter().flat_map(|entity| entity.id.iter().flatten().cloned()).collect();
        (
            if ids.is_empty() { None } else { Some(ids) },
            None,
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

    fn geo_query() -> OptionalGeoQuery {
        serde_urlencoded::from_str("georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B13.4%2C52.5%5D&geoproperty=location")
            .expect("valid geo query")
    }

    #[test]
    fn query_body_maps_every_selection_member_without_a_lossy_fallback() {
        // The POST body form of a query maps field for field onto the GET
        // parameter form. A body that carries filters must keep them: the old
        // URL round-trip dropped `entityMapLifetime` entirely and replaced the
        // whole selection with an unfiltered `local=true` query on any encode or
        // parse hiccup.
        let body = QueryRequestBody {
            entities: Some(vec![EntitySelector {
                id: Some(vec!["urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")]),
                id_pattern: None,
                type_: Some("Vehicle".parse().unwrap()),
            }]),
            attrs: Some(vec!["speed".parse().unwrap(), "brandName".parse().unwrap()]),
            q: Some("speed>50".parse().unwrap()),
            geo: geo_query(),
            scope_q: Some("/Madrid/Gardens".parse().expect("valid scope query")),
            entity_map_lifetime: Some("PT1H".to_string()),
        };
        let params = query_body_to_params(&body);
        pretty_assertions::assert_eq!(params.id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ngsi-ld:Vehicle:1"));
        assert!(params.entity_type.is_none());
        pretty_assertions::assert_eq!(
            params.attrs.as_deref().map(|names| names.iter().map(AttributeName::as_str).collect::<Vec<_>>()),
            Some(vec!["speed", "brandName"])
        );
        pretty_assertions::assert_eq!(params.q.as_ref().map(ToString::to_string).as_deref(), Some("speed>50"));
        assert!(params.geo.0.is_some());
        pretty_assertions::assert_eq!(params.scope_q, Some("/Madrid/Gardens".parse().expect("valid scope query")));
        pretty_assertions::assert_eq!(params.entity_map_lifetime.as_deref(), Some("PT1H"));
        pretty_assertions::assert_eq!(params.local, Some(true));
    }

    #[test]
    fn query_body_with_no_entities_still_localises() {
        let body = QueryRequestBody {
            entities: None,
            attrs: None,
            q: Some("speed>50".parse().unwrap()),
            geo: OptionalGeoQuery::default(),
            scope_q: None,
            entity_map_lifetime: None,
        };
        let params = query_body_to_params(&body);
        assert!(params.id.is_none());
        assert!(params.entity_type.is_none());
        pretty_assertions::assert_eq!(params.q.as_ref().map(ToString::to_string).as_deref(), Some("speed>50"));
        pretty_assertions::assert_eq!(params.local, Some(true));
    }
}
