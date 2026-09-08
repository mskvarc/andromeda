//! Pure evaluation of compiled geo and scope queries against expanded entities.

use crate::expanded::{entity_member::extract_scopes, geo_property::geo_property_instance_geometries};
use andromeda_ngsi_ld_geometry::relation::matches;
use andromeda_ngsi_ld_query::{geo::query::GeoQuery, scope::query::ScopeQuery};
use serde_json::Value;

/// Whether an expanded entity satisfies a compiled geo-query from clause 4.10.
///
/// Where the target `GeoProperty` has several `datasetId` instances, one of them
/// meeting the restriction is enough, and an entity conveying no instance of it
/// matches nothing. This is the same reading the SQL rendering gives the clause
/// with its correlated `EXISTS`, so a query and the equivalent subscription
/// cannot disagree about a multi-instance `GeoProperty`.
#[must_use]
pub fn matches_geo_query(geo_query: &GeoQuery, expanded: &Value) -> bool {
    geo_property_instance_geometries(expanded, geo_query.geoproperty.as_str()).any(|subject| matches(&geo_query.georel, &subject, &geo_query.geometry))
}

/// Whether an expanded entity's scopes satisfy a parsed `scopeQ` from clause 4.18.
#[must_use]
pub fn matches_scope_query(query: &ScopeQuery, expanded: &Value) -> bool {
    query.matches(&extract_scopes(expanded))
}

#[cfg(test)]
mod tests {
    use crate::expanded::query_match::matches_geo_query;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry};
    use andromeda_ngsi_ld_query::geo::query::GeoQuery;
    use serde_json::{Value, json};

    const CUSTOM_GEOPROPERTY: &str = "https://example.com/observationPoint";

    /// A `within` query over a small box around `(13.4, 52.5)`.
    fn within_berlin(geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Within,
            geometry: Geometry::from_geojson_value(&json!({
                "type": "Polygon",
                "coordinates": [[[13.0, 52.0], [14.0, 52.0], [14.0, 53.0], [13.0, 53.0], [13.0, 52.0]]]
            }))
            .expect("polygon parses"),
            geoproperty: geoproperty.parse().expect("valid attribute name"),
        }
    }

    /// An entity whose `GeoProperty` carries the given instances, each as
    /// `(datasetId, coordinates)` with `None` for the default instance.
    fn entity_with_instances(geoproperty: &str, instances: &[(Option<&str>, [f64; 2])]) -> Value {
        let rendered: Vec<Value> = instances
            .iter()
            .map(|(dataset_id, coordinates)| {
                let mut instance = json!({
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{ "@value": {"type": "Point", "coordinates": coordinates} }]
                });
                if let Some(dataset_id) = dataset_id {
                    instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset_id}]);
                }
                instance
            })
            .collect();
        json!([{ "@id": "urn:ngsi-ld:Building:001", geoproperty: rendered }])
    }

    /// Clause 4.10: where a `GeoProperty` has several `datasetId` instances, one
    /// of them meeting the restriction is enough. The default instance sits
    /// outside the reference polygon here, so a matcher that read only the
    /// default one would report no match where the SQL rendering's correlated
    /// `EXISTS` reports one.
    #[test]
    fn a_tagged_instance_matching_the_restriction_is_enough_when_the_default_does_not() {
        let entity = entity_with_instances(CUSTOM_GEOPROPERTY, &[(None, [0.0, 0.0]), (Some("urn:ngsi-ld:Dataset:sensor-a"), [13.4, 52.5])]);
        assert!(matches_geo_query(&within_berlin(CUSTOM_GEOPROPERTY), &entity));
    }

    /// The converse ordering must give the same answer: which instance carries
    /// the matching geometry is not what decides.
    #[test]
    fn the_default_instance_matching_the_restriction_is_enough_when_a_tagged_one_does_not() {
        let entity = entity_with_instances(CUSTOM_GEOPROPERTY, &[(Some("urn:ngsi-ld:Dataset:sensor-a"), [0.0, 0.0]), (None, [13.4, 52.5])]);
        assert!(matches_geo_query(&within_berlin(CUSTOM_GEOPROPERTY), &entity));
    }

    #[test]
    fn an_entity_no_instance_of_which_meets_the_restriction_does_not_match() {
        let entity = entity_with_instances(CUSTOM_GEOPROPERTY, &[(None, [0.0, 0.0]), (Some("urn:ngsi-ld:Dataset:sensor-a"), [1.0, 1.0])]);
        assert!(!matches_geo_query(&within_berlin(CUSTOM_GEOPROPERTY), &entity));
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
    /// non-matching, whatever the relation.
    #[test]
    fn an_entity_conveying_no_instance_of_the_target_geoproperty_does_not_match() {
        let entity = entity_with_instances("https://example.com/somethingElse", &[(None, [13.4, 52.5])]);
        assert!(!matches_geo_query(&within_berlin(CUSTOM_GEOPROPERTY), &entity));
    }
}
