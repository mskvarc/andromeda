/// Prefer / Preference-Applied header handling (NGSI-LD clause 6.3.6).
///
/// Tests GeoJSON body preference (`body=json` / `body=ld+json`) and version
/// conformance negotiation (`ngsi-ld=<version>`) with 203 + Preference-Applied.
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::prefer_header";

fn geo_headers() -> RequestHeaders {
    RequestHeaders::new().accept_geo_json().link_context(fixtures::test_suite_context_url())
}

fn json_headers() -> RequestHeaders {
    RequestHeaders::new().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── GeoJSON body preference ───────────────────────────────────

    trials.push(trial(
        &format!("{PREFIX}::retrieve_geojson_default_includes_context_in_body"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:prefer1");
            let entity = serde_json::json!({
                "id": id,
                "type": "Building",
                "location": {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            let params = QueryParams::new().geometry_property("location");
            let resp = ctx.client.get_entity(&id, &params, geo_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert!(body.get("@context").is_some(), "default GeoJSON should include @context in body");
        },
    ));

    trials.push(trial(
        &format!("{PREFIX}::retrieve_geojson_body_ld_json_includes_context_in_body"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:prefer2");
            let entity = serde_json::json!({
                "id": id,
                "type": "Building",
                "location": {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            let headers = geo_headers().prefer("body=ld+json");
            let params = QueryParams::new().geometry_property("location");
            let resp = ctx.client.get_entity(&id, &params, headers).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert!(body.get("@context").is_some(), "body=ld+json should include @context in body");
        },
    ));

    trials.push(trial(
        &format!("{PREFIX}::retrieve_geojson_body_json_moves_context_to_link"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Building:prefer3");
            let entity = serde_json::json!({
                "id": id,
                "type": "Building",
                "location": {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
                },
                "@context": [fixtures::test_suite_context_url()]
            });
            ctx.client.create_entity(&entity).await;

            let headers = geo_headers().prefer("body=json");
            let params = QueryParams::new().geometry_property("location");
            let resp = ctx.client.get_entity(&id, &params, headers).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            assert!(body.get("@context").is_none(), "body=json should NOT include @context in body");
            assert!(resp.headers.get("link").is_some(), "body=json should have Link header with context");
        },
    ));

    trials.push(trial(&format!("{PREFIX}::query_geojson_body_json_moves_context_to_link"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:prefer4");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [2.3522, 48.8566]}
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let headers = geo_headers().prefer("body=json");
        let params = QueryParams::new().type_("Building");
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert_eq!(body["type"], "FeatureCollection");
        assert!(body.get("@context").is_none(), "body=json should NOT include @context in FeatureCollection");
        assert!(resp.headers.get("link").is_some(), "body=json should have Link header");
    }));

    // ── Version conformance ───────────────────────────────────────

    trials.push(trial(&format!("{PREFIX}::retrieve_version_prefer_returns_203"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:prefer5");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "temperature": {"type": "Property", "value": 25.0},
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let headers = json_headers().prefer("ngsi-ld=1.3");
        let params = QueryParams::new();
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        assert_eq!(resp.status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
        let pref_applied = resp.headers.get("preference-applied").and_then(|v| v.to_str().ok());
        assert_eq!(pref_applied, Some("ngsi-ld=1.9"));
    }));

    trials.push(trial(&format!("{PREFIX}::query_version_prefer_returns_203"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:prefer6");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "temperature": {"type": "Property", "value": 25.0},
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let headers = json_headers().prefer("ngsi-ld=1.4");
        let params = QueryParams::new().type_("Building");
        let resp = ctx.client.query_entities(&params, headers).await;
        assert_eq!(resp.status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
        let pref_applied = resp.headers.get("preference-applied").and_then(|v| v.to_str().ok());
        assert_eq!(pref_applied, Some("ngsi-ld=1.9"));
    }));

    trials.push(trial(&format!("{PREFIX}::no_prefer_returns_200_no_preference_applied"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:prefer7");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "temperature": {"type": "Property", "value": 25.0},
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let headers = json_headers();
        let params = QueryParams::new();
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        assert_eq!(resp.status, StatusCode::OK);
        assert!(
            resp.headers.get("preference-applied").is_none(),
            "should not have Preference-Applied without Prefer"
        );
    }));

    trials.push(trial(&format!("{PREFIX}::combined_body_json_and_version_prefer"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:prefer8");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let headers = geo_headers().prefer("body=json, ngsi-ld=1.3");
        let params = QueryParams::new().geometry_property("location");
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        assert_eq!(resp.status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
        let body: serde_json::Value = resp.json();
        assert!(body.get("@context").is_none(), "body=json should remove @context from body");
        assert!(resp.headers.get("link").is_some(), "should have Link header");
        let pref_applied = resp.headers.get("preference-applied").and_then(|v| v.to_str().ok());
        assert_eq!(pref_applied, Some("ngsi-ld=1.9"));
    }));

    // ── Version adaptation payload tests ──────────────────────────

    trials.push(trial(&format!("{PREFIX}::version_1_0_removes_dataset_id_from_properties"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:prefer9");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "datasetId": "urn:ngsi-ld:Dataset:temp1"
            },
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let headers = json_headers().prefer("ngsi-ld=1.0");
        let params = QueryParams::new();
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        assert_eq!(resp.status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
        let body: serde_json::Value = resp.json();
        // datasetId should have been removed for v1.0 compatibility
        let temp = &body["https://uri.fiware.org/ns/data-models#temperature"];
        assert!(temp.get("datasetId").is_none(), "datasetId should be removed for v1.0");
    }));

    trials.push(trial(&format!("{PREFIX}::version_1_0_removes_scope_from_entity"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:prefer10");
        let entity = serde_json::json!({
            "id": id,
            "type": "Building",
            "scope": ["/A/B"],
            "temperature": {"type": "Property", "value": 25.0},
            "@context": [fixtures::test_suite_context_url()]
        });
        ctx.client.create_entity(&entity).await;

        let headers = json_headers().prefer("ngsi-ld=1.0");
        let params = QueryParams::new();
        let resp = ctx.client.get_entity(&id, &params, headers).await;
        assert_eq!(resp.status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
        let body: serde_json::Value = resp.json();
        assert!(body.get("scope").is_none(), "scope should be removed for v1.0");
    }));
}
