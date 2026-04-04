/// Entity ordering with orderFrom/orderGeometry (NGSI-LD S4.23, S5.2.43, S6.4.3.2).
///
/// Verifies distance-based ordering using explicit reference coordinates (orderFrom),
/// geometry type override (orderGeometry), geo-query fallback, precedence rules,
/// and error responses for invalid or missing parameters.
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial_clean_db,
};
use libtest_mimic::Trial;
use reqwest::StatusCode;

const PREFIX: &str = "spec_features::entity_ordering";

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

/// Create three Building entities at known coordinates for distance ordering tests.
/// - Entity A: Berlin north `[13.3986, 52.5547]`
/// - Entity B: Berlin south `[13.509818, 52.363457]`
/// - Entity C: Paris `[2.3522, 48.8566]`
async fn create_test_entities(ctx: &crate::harness::TestCtx) -> (String, String, String) {
    let id_a = ctx.entity_id("urn:ngsi-ld:Building:order-A");
    let id_b = ctx.entity_id("urn:ngsi-ld:Building:order-B");
    let id_c = ctx.entity_id("urn:ngsi-ld:Building:order-C");

    let entity_a = serde_json::json!({
        "id": id_a,
        "type": "Building",
        "name": {"type": "Property", "value": "Alpha"},
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
        },
        "@context": [fixtures::test_suite_context_url()]
    });
    let entity_b = serde_json::json!({
        "id": id_b,
        "type": "Building",
        "name": {"type": "Property", "value": "Beta"},
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.509818, 52.363457]}
        },
        "@context": [fixtures::test_suite_context_url()]
    });
    let entity_c = serde_json::json!({
        "id": id_c,
        "type": "Building",
        "name": {"type": "Property", "value": "Gamma"},
        "location": {
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [2.3522, 48.8566]}
        },
        "@context": [fixtures::test_suite_context_url()]
    });

    let resp = ctx.client.create_entity(&entity_a).await;
    assert_eq!(resp.status, StatusCode::CREATED, "create entity A");
    let resp = ctx.client.create_entity(&entity_b).await;
    assert_eq!(resp.status, StatusCode::CREATED, "create entity B");
    let resp = ctx.client.create_entity(&entity_c).await;
    assert_eq!(resp.status, StatusCode::CREATED, "create entity C");

    (id_a, id_b, id_c)
}

fn extract_ids(body: &serde_json::Value) -> Vec<String> {
    body.as_array()
        .expect("response body should be an array")
        .iter()
        .map(|e| e["id"].as_str().expect("entity should have id").to_string())
        .collect()
}

pub fn tests(trials: &mut Vec<Trial>) {
    // Test 1: dist-asc with orderFrom Point (S4.23, S5.2.43)
    trials.push(trial_clean_db(&format!("{PREFIX}::dist_asc_with_order_from_point"), |ctx| async move {
        let (id_a, id_b, id_c) = create_test_entities(&ctx).await;
        let params = QueryParams::new()
            .type_("Building")
            .order_by("location;dist-asc")
            .order_from("[13.3986,52.5547]");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let ids = extract_ids(&resp.json_value());
        assert_eq!(ids, vec![id_a, id_b, id_c], "should be ordered nearest to farthest from A");
    }));

    // Test 2: dist-desc with orderFrom Point (S4.23)
    trials.push(trial_clean_db(&format!("{PREFIX}::dist_desc_with_order_from_point"), |ctx| async move {
        let (id_a, id_b, id_c) = create_test_entities(&ctx).await;
        let params = QueryParams::new()
            .type_("Building")
            .order_by("location;dist-desc")
            .order_from("[13.3986,52.5547]");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let ids = extract_ids(&resp.json_value());
        assert_eq!(ids, vec![id_c, id_b, id_a], "should be ordered farthest to nearest from A");
    }));

    // Test 3: dist-asc with orderFrom and orderGeometry=LineString (S4.23.3 ex.10)
    trials.push(trial_clean_db(&format!("{PREFIX}::dist_asc_with_order_from_and_geometry"), |ctx| async move {
        let (id_a, id_b, id_c) = create_test_entities(&ctx).await;
        // LineString near Paris -- C should be closest
        let params = QueryParams::new()
            .type_("Building")
            .order_by("location;dist-asc")
            .order_from("[[2.3,48.8],[2.4,48.9]]")
            .order_geometry("LineString");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let ids = extract_ids(&resp.json_value());
        // C (Paris) is nearest to the Paris-area LineString
        assert_eq!(ids[0], id_c, "C should be nearest to the Paris-area LineString");
        // B (Berlin south, lat 52.36) is closer to Paris than A (Berlin north, lat 52.55)
        assert_eq!(ids[1], id_b, "B should be second (closer to Paris than A)");
        assert_eq!(ids[2], id_a, "A should be farthest");
    }));

    // Test 4: dist-asc with geo-query fallback (S4.23)
    trials.push(trial_clean_db(&format!("{PREFIX}::dist_asc_with_geo_query_fallback"), |ctx| async move {
        let (id_a, id_b, _id_c) = create_test_entities(&ctx).await;
        // Geo query centered on Berlin, large radius to include A and B but not C
        let params = QueryParams::new()
            .type_("Building")
            .georel("near;maxDistance==50000")
            .geometry("Point")
            .coordinates("[13.3986,52.5547]")
            .order_by("location;dist-asc");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let ids = extract_ids(&resp.json_value());
        // Only A and B within 50km of Berlin, ordered by distance
        assert!(ids.len() == 2, "should return 2 entities within 50km");
        assert_eq!(ids[0], id_a, "A should be nearest");
        assert_eq!(ids[1], id_b, "B should be second");
    }));

    // Test 5: distance ordering without reference returns BadRequestData (S5.7.2.4)
    trials.push(trial_clean_db(
        &format!("{PREFIX}::dist_ordering_without_reference_returns_error"),
        |ctx| async move {
            create_test_entities(&ctx).await;
            let params = QueryParams::new().type_("Building").order_by("location;dist-asc");
            let resp = ctx.client.query_entities(&params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::BAD_REQUEST);
            let body = resp.json_value();
            assert_eq!(body["type"].as_str().unwrap(), "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
        },
    ));

    // Test 6: orderFrom takes precedence over geo-query (S5.2.43)
    trials.push(trial_clean_db(
        &format!("{PREFIX}::order_from_takes_precedence_over_geo_query"),
        |ctx| async move {
            let (id_a, id_b, _id_c) = create_test_entities(&ctx).await;
            // Geo filter: near Paris (large radius to include Berlin entities)
            // orderFrom: Berlin -- ordering should use Berlin reference
            let params = QueryParams::new()
                .type_("Building")
                .georel("near;maxDistance==1000000")
                .geometry("Point")
                .coordinates("[2.3522,48.8566]")
                .order_by("location;dist-asc")
                .order_from("[13.3986,52.5547]");
            let resp = ctx.client.query_entities(&params, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let ids = extract_ids(&resp.json_value());
            // Ordering uses Berlin point: A first, B second
            assert!(ids.len() >= 2, "should return at least 2 entities");
            assert_eq!(ids[0], id_a, "A should be first (nearest to Berlin orderFrom)");
            assert_eq!(ids[1], id_b, "B should be second");
        },
    ));

    // Test 7: invalid orderFrom returns BadRequestData (S5.7.2.4)
    trials.push(trial_clean_db(&format!("{PREFIX}::invalid_order_from_returns_error"), |ctx| async move {
        let params = QueryParams::new().type_("Building").order_by("location;dist-asc").order_from("notjson");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // Test 8: orderGeometry=GeometryCollection returns BadRequestData (S5.2.43)
    trials.push(trial_clean_db(&format!("{PREFIX}::invalid_order_geometry_returns_error"), |ctx| async move {
        let params = QueryParams::new()
            .type_("Building")
            .order_by("location;dist-asc")
            .order_from("[8,40]")
            .order_geometry("GeometryCollection");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
    }));

    // Test 9: mixed distance and value ordering (S4.23.3)
    trials.push(trial_clean_db(&format!("{PREFIX}::mixed_distance_and_value_ordering"), |ctx| async move {
        let (id_a, id_b, id_c) = create_test_entities(&ctx).await;
        // Order by name ascending first, then by distance
        let params = QueryParams::new()
            .type_("Building")
            .order_by("name;asc,location;dist-asc")
            .order_from("[13.3986,52.5547]");
        let resp = ctx.client.query_entities(&params, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let ids = extract_ids(&resp.json_value());
        // Names: Alpha (A), Beta (B), Gamma (C) -- alphabetical order
        assert_eq!(ids, vec![id_a, id_b, id_c], "should be ordered by name ascending");
    }));
}
