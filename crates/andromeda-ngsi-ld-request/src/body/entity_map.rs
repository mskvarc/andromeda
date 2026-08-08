//! HTTP request model types for entity map query endpoints.

use crate::query::deserialization::{OptionalGeoQuery, deserialize_optional_filter_expr, deserialize_optional_iso8601_duration};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::{filter::expression::Expr, pattern::Pattern, scope::query::ScopeQuery, type_selection::grammar::TypeExprRaw};
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
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
    #[serde(default, deserialize_with = "deserialize_optional_iso8601_duration")]
    pub entity_map_lifetime: Option<Iso8601Duration>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::entity::body_conversion::query_body_to_params;

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
            entity_map_lifetime: Some("PT1H".parse().expect("valid duration")),
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
        pretty_assertions::assert_eq!(params.entity_map_lifetime.as_ref().map(ToString::to_string).as_deref(), Some("PT1H"));
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
