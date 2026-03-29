/// Feature 2: geometryProperty on GET /entities collection (spec 6.4.3.2)
///
/// Verifies that geometryProperty is respected in collection queries with
/// GeoJSON Accept, not just single-entity GET.
use crate::harness::{
    self,
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::geometry_property_collection";

fn geo_headers() -> RequestHeaders {
    RequestHeaders::new().accept_geo_json().link_context(fixtures::test_suite_context_url())
}

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── geometryProperty on collection query with geo+json ────────
    trials.push(trial(&format!("{PREFIX}::custom_geometry_property_in_collection"), |ctx| async move {
        // Create entity with a custom GeoProperty
        let id = ctx.entity_id("urn:ngsi-ld:Building:gpc1");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "observationPoint": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
            },
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [0.0, 0.0]}
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Query with geometryProperty=observationPoint
        let params = QueryParams::new().type_("Building").geometry_property("observationPoint");
        let resp = ctx.client.query_entities(&params, geo_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "FeatureCollection");
        let features = body["features"].as_array().expect("features array");
        assert!(!features.is_empty(), "should have at least one feature");

        let geom = &features[0]["geometry"];
        assert_eq!(geom["type"], "Point");
        let coords = geom["coordinates"].as_array().expect("coordinates");
        // Should use observationPoint coords (13.3986, 52.5547), not location (0,0)
        let lon = coords[0].as_f64().unwrap();
        assert!((lon - 13.3986).abs() < 0.001, "should use observationPoint, got lon={lon}");
    }));

    // ── default geometryProperty (location) in collection ─────────
    trials.push(trial(&format!("{PREFIX}::default_geometry_property_is_location"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:gpc2");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [2.3522, 48.8566]}
            },
            "otherGeo": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [99.0, 99.0]}
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Query without geometryProperty → uses location
        let params = QueryParams::new().type_("Building");
        let resp = ctx.client.query_entities(&params, geo_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let features = body["features"].as_array().expect("features array");
        assert!(!features.is_empty());

        let coords = &features[0]["geometry"]["coordinates"];
        let lon = coords[0].as_f64().unwrap();
        assert!((lon - 2.3522).abs() < 0.001, "should use location by default, got lon={lon}");
    }));
}
