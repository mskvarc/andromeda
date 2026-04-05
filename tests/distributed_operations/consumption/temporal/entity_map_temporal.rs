use crate::{
    distributed_operations::prepare_registration,
    harness::{
        client::{QueryParams, RequestHeaders},
        fixtures,
        mock_server::MockServer,
        trial::trial,
    },
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "distributed_operations::consumption::temporal::entity_map_temporal";
const TEMPORAL_VEHICLE_FIXTURE: &str = "temporal_entities/2020-08-vehicle-temporal-representation.jsonld";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";

fn ld_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

fn ctx_headers() -> RequestHeaders {
    RequestHeaders::new().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // Test 1: temporal_query_creates_entity_map
    // Spec ref: 5.7.4.4 -- entityMap=true creates a new EntityMap and returns 201
    trials.push(trial(&format!("{PREFIX}::temporal_query_creates_entity_map"), |ctx| async move {
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_create");
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new()
                    .type_("Vehicle")
                    .timerel("after")
                    .timeat("2020-01-01T00:00:00Z")
                    .entity_map(true),
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED, "entityMap=true should yield 201 Created");

        let map_header = resp.headers.get("NGSILD-EntityMap").expect("NGSILD-EntityMap header must be present");
        let map_id = map_header.to_str().unwrap();
        assert!(map_id.starts_with("urn:ngsi-ld:EntityMap:"), "EntityMap ID should be a URN");
        ctx.track_entity_map(map_id);
    }));

    // Test 2: temporal_query_scoped_by_entity_map
    // Spec ref: 5.5.14 -- subsequent requests with NGSILD-EntityMap header are
    // filtered to only consider the entities listed in the map
    trials.push(trial(&format!("{PREFIX}::temporal_query_scoped_by_entity_map"), |ctx| async move {
        let entity_a = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_scope_a");
        let entity_b = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_scope_b");

        let fixture_a = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_a);
        let fixture_b = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_b);
        assert_eq!(ctx.client.create_temporal_entity(&fixture_a).await.status, StatusCode::CREATED);
        assert_eq!(ctx.client.create_temporal_entity(&fixture_b).await.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_a);
        ctx.track_temporal_entity(&entity_b);

        // Create EntityMap containing A and B
        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new()
                    .type_("Vehicle")
                    .timerel("after")
                    .timeat("2020-01-01T00:00:00Z")
                    .entity_map(true),
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let map_id = resp.headers.get("NGSILD-EntityMap").unwrap().to_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        // Create a third entity after the map was frozen
        let entity_c = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_scope_c");
        let fixture_c = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_c);
        assert_eq!(ctx.client.create_temporal_entity(&fixture_c).await.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_c);

        // Query with the frozen EntityMap -- only A and B should appear
        let resp = ctx
            .client
            .query_temporal_entities_with_entity_map(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z"),
                &map_id,
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        let ids: Vec<&str> = body.iter().filter_map(|e| e["id"].as_str()).collect();
        assert!(ids.contains(&entity_a.as_str()), "entity A should be in scoped results");
        assert!(ids.contains(&entity_b.as_str()), "entity B should be in scoped results");
        assert!(!ids.contains(&entity_c.as_str()), "entity C should NOT be in scoped results (not in map)");
    }));

    // Test 3: temporal_query_entity_map_echoed_in_response
    // Spec ref: 5.7.4 -- the location of the EntityMap used in the operation shall
    // be returned in a specific field (NGSILD-EntityMap response header)
    trials.push(trial(&format!("{PREFIX}::temporal_query_entity_map_echoed_in_response"), |ctx| async move {
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_echo");
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        assert_eq!(ctx.client.create_temporal_entity(&entity).await.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        // Create EntityMap
        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new()
                    .type_("Vehicle")
                    .timerel("after")
                    .timeat("2020-01-01T00:00:00Z")
                    .entity_map(true),
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let map_id = resp.headers.get("NGSILD-EntityMap").unwrap().to_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        // Query again with the EntityMap header
        let resp = ctx
            .client
            .query_temporal_entities_with_entity_map(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z"),
                &map_id,
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let echoed = resp
            .headers
            .get("NGSILD-EntityMap")
            .expect("NGSILD-EntityMap should be echoed in response")
            .to_str()
            .unwrap();
        assert_eq!(echoed, map_id, "echoed EntityMap ID should match the one sent");
    }));

    // Test 4: temporal_retrieve_scoped_by_entity_map
    // Spec ref: 5.7.3.4 -- entity not in map returns 404
    trials.push(trial(&format!("{PREFIX}::temporal_retrieve_scoped_by_entity_map"), |ctx| async move {
        let entity_a = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tr_scope_a");
        let fixture_a = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_a);
        assert_eq!(ctx.client.create_temporal_entity(&fixture_a).await.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_a);

        // Create EntityMap containing only entity A
        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new()
                    .type_("Vehicle")
                    .timerel("after")
                    .timeat("2020-01-01T00:00:00Z")
                    .entity_map(true),
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let map_id = resp.headers.get("NGSILD-EntityMap").unwrap().to_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        // Create entity B (not in the map)
        let entity_b = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tr_scope_b");
        let fixture_b = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_b);
        assert_eq!(ctx.client.create_temporal_entity(&fixture_b).await.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_b);

        // Retrieve entity B using the EntityMap that only contains A
        let resp = ctx
            .client
            .get_temporal_entity_with_entity_map(&entity_b, &QueryParams::new(), &map_id, ld_headers())
            .await;
        assert_eq!(resp.status, StatusCode::NOT_FOUND, "entity B is not in the map, should be 404");
    }));

    // Test 5: temporal_retrieve_passes_with_entity_map
    // Spec ref: 5.7.3.4 -- entity in map returns 200 with NGSILD-EntityMap echoed
    trials.push(trial(&format!("{PREFIX}::temporal_retrieve_passes_with_entity_map"), |ctx| async move {
        let entity_a = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tr_pass");
        let fixture_a = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_a);
        assert_eq!(ctx.client.create_temporal_entity(&fixture_a).await.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_a);

        // Create EntityMap containing entity A
        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new()
                    .type_("Vehicle")
                    .timerel("after")
                    .timeat("2020-01-01T00:00:00Z")
                    .entity_map(true),
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let map_id = resp.headers.get("NGSILD-EntityMap").unwrap().to_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        // Retrieve entity A using the EntityMap
        let resp = ctx
            .client
            .get_temporal_entity_with_entity_map(&entity_a, &QueryParams::new(), &map_id, ld_headers())
            .await;
        assert_eq!(resp.status, StatusCode::OK, "entity A is in the map, should return 200");

        let body: serde_json::Value = resp.json();
        assert_eq!(body["id"].as_str(), Some(entity_a.as_str()));

        let echoed = resp
            .headers
            .get("NGSILD-EntityMap")
            .expect("NGSILD-EntityMap should be echoed in response")
            .to_str()
            .unwrap();
        assert_eq!(echoed, map_id, "echoed EntityMap ID should match");
    }));

    // Test 6: temporal_query_entity_map_with_lifetime
    // Spec ref: 5.5.14 -- entityMapLifetime suggests an expiry duration
    trials.push(trial(&format!("{PREFIX}::temporal_query_entity_map_with_lifetime"), |ctx| async move {
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_lifetime");
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        assert_eq!(ctx.client.create_temporal_entity(&entity).await.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new()
                    .type_("Vehicle")
                    .timerel("after")
                    .timeat("2020-01-01T00:00:00Z")
                    .entity_map(true)
                    .entity_map_lifetime("PT5M"),
                ld_headers(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let map_id = resp.headers.get("NGSILD-EntityMap").unwrap().to_str().unwrap().to_string();
        ctx.track_entity_map(&map_id);

        // Retrieve the EntityMap via the dedicated endpoint
        let resp = ctx.client.get_entity_map(&map_id, ctx_headers()).await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        let expires_at_str = body["expiresAt"].as_str().expect("expiresAt should be present");
        let expires_at = chrono::DateTime::parse_from_rfc3339(expires_at_str).expect("expiresAt should be valid RFC3339");
        let now = chrono::Utc::now();
        let diff = expires_at.signed_duration_since(now);
        assert!(
            diff.num_seconds() > 180 && diff.num_seconds() <= 360,
            "expiresAt should be approximately 5 minutes from now, got {} seconds",
            diff.num_seconds()
        );
    }));

    // Test 7: temporal_query_entity_map_with_distributed_merge
    // Spec ref: 5.5.14, 5.7.4.4 -- EntityMap captures both local and remote entity IDs
    trials.push(trial(
        &format!("{PREFIX}::temporal_query_entity_map_with_distributed_merge"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let local_entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_dist_local");
            let remote_entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:em_tq_dist_remote");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:em_tq_dist");

            // Create local temporal entity
            let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &local_entity_id);
            assert_eq!(ctx.client.create_temporal_entity(&entity).await.status, StatusCode::CREATED);
            ctx.track_temporal_entity(&local_entity_id);

            // Create CSR pointing to mock
            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &remote_entity_id, &mock.url(""), None, None, None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Mock returns a remote temporal entity
            let mock_entity = serde_json::json!({
                "id": remote_entity_id,
                "type": "Vehicle",
                "speed": [{"type": "Property", "value": 80, "observedAt": "2020-08-01T12:00:00Z"}]
            });
            let mock_array = serde_json::Value::Array(vec![mock_entity]);
            mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_array).await;

            // Query with entityMap=true -- should capture both local and remote IDs
            let resp = ctx
                .client
                .query_temporal_entities(
                    &QueryParams::new()
                        .type_("Vehicle")
                        .timerel("after")
                        .timeat("2020-01-01T00:00:00Z")
                        .entity_map(true),
                    ld_headers(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);
            let map_id = resp.headers.get("NGSILD-EntityMap").unwrap().to_str().unwrap().to_string();
            ctx.track_entity_map(&map_id);

            // Retrieve the EntityMap and verify both IDs are present
            let resp = ctx.client.get_entity_map(&map_id, ctx_headers()).await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: serde_json::Value = resp.json();
            let entity_map = body["entityMap"].as_object().expect("entityMap should be an object");
            assert!(entity_map.contains_key(local_entity_id.as_str()), "EntityMap should contain local entity ID");

            mock.stop();
        },
    ));
}
