//! Evaluating a clause 4.10 geo-query against an expanded entity payload.
//!
//! The reference reading of `georel` is
//! [`andromeda_ngsi_ld_geometry::relation::matches_geo`], and reading a named
//! GeoProperty off an expanded payload is
//! [`extract_geo_property_geometry`]. This is the one place the two are joined,
//! so subscription matching, interval notification and residual filtering all
//! answer a geo-query the same way.
//!
//! [`extract_geo_property_geometry`]: andromeda_ngsi_ld_model::expanded::geo_property::extract_geo_property_geometry

use andromeda_ngsi_ld_geometry::relation::matches_geo;
use andromeda_ngsi_ld_model::expanded::geo_property::extract_geo_property_geometry;
use andromeda_ngsi_ld_query::geo::query::GeoQuery;
use serde_json::Value;

/// Whether an expanded entity satisfies a compiled geo-query (clause 4.10).
///
/// An entity that carries no instance of the named GeoProperty does not
/// satisfy any `georel`: clause 4.10 evaluates a spatial relationship between
/// two geometries, and an absent one has no relationship to evaluate.
pub fn matches_geo_query(geo_query: &GeoQuery, expanded: &Value) -> bool {
    let Some(subject) = extract_geo_property_geometry(expanded, geo_query.geoproperty_iri.as_str()) else {
        return false;
    };
    matches_geo(&geo_query.georel, subject.as_ref(), geo_query.geometry.as_ref())
}

#[cfg(test)]
mod tests {
    use crate::service::geo_filter::matches_geo_query;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::geo::query::GeoQuery;
    use serde_json::json;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn near_berlin(max_distance_m: f64) -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(max_distance_m),
                min_distance: None,
            },
            geometry: Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid reference geometry"),
            geoproperty_iri: LOCATION.parse().expect("valid IRI"),
        }
    }

    fn at(longitude: f64, latitude: f64) -> serde_json::Value {
        json!([{
            "@id": "urn:ngsi-ld:Building:001",
            LOCATION: [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@value": {"type": "Point", "coordinates": [longitude, latitude]}
                }]
            }]
        }])
    }

    #[test]
    fn an_entity_inside_the_reference_distance_satisfies_the_geo_query() {
        assert!(matches_geo_query(&near_berlin(1000.0), &at(13.4, 52.5)));
    }

    #[test]
    fn an_entity_outside_the_reference_distance_does_not_satisfy_the_geo_query() {
        assert!(!matches_geo_query(&near_berlin(1000.0), &at(13.4, 53.5)));
    }

    #[test]
    fn an_entity_that_carries_no_such_geoproperty_does_not_satisfy_the_geo_query() {
        let entity = json!([{"@id": "urn:ngsi-ld:Building:001"}]);
        assert!(!matches_geo_query(&near_berlin(1000.0), &entity));
    }
}
