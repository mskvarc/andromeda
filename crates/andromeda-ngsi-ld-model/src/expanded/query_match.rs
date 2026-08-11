//! Pure evaluation of compiled geo and scope queries against expanded entities.

use crate::expanded::{entity_member::extract_scopes, geo_property::extract_geo_property_geometry};
use andromeda_ngsi_ld_geometry::relation::matches;
use andromeda_ngsi_ld_query::{geo::query::GeoQuery, scope::query::ScopeQuery};
use serde_json::Value;

/// Whether an expanded entity satisfies a compiled geo-query from clause 4.10.
pub fn matches_geo_query(geo_query: &GeoQuery, expanded: &Value) -> bool {
    let Some(subject) = extract_geo_property_geometry(expanded, geo_query.geoproperty_iri.as_str()) else {
        return false;
    };
    matches(&geo_query.georel, &subject, &geo_query.geometry)
}

/// Whether an expanded entity's scopes satisfy a parsed `scopeQ` from clause 4.18.
pub fn matches_scope_query(query: &ScopeQuery, expanded: &Value) -> bool {
    query.matches(&extract_scopes(expanded))
}
