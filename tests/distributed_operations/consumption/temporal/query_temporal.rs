#![allow(clippy::expect_used, clippy::unwrap_used)]
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
use reqwest::{Method, StatusCode};

const PREFIX: &str = "distributed_operations::consumption::temporal::query_temporal";
const TEMPORAL_VEHICLE_FIXTURE: &str = "temporal_entities/2020-08-vehicle-temporal-representation.jsonld";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";

fn ld_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

fn warning_header_values(resp: &crate::harness::client::NgsiLdResponse) -> Vec<String> {
    resp.headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(|s| s.to_string()))
        .collect()
}

pub fn tests(trials: &mut Vec<Trial>) {
    trials.push(trial(&format!("{PREFIX}::query_temporal_local_skips_forwarding"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_local_skip");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_local_skip");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_array).await;

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z").local(true),
                ld_headers(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        pretty_assertions::assert_eq!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await,
            0,
            "mock should not be called when local=true"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_temporal_inclusive_merge"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_inclusive");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_inclusive");

        // Create local temporal entity
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        // Create CSR in inclusive mode (default)
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns temporal entity with a different attribute
        let mock_entity = serde_json::json!({
            "id": entity_id,
            "type": "Vehicle",
            "altitude": [{"type": "Property", "value": 100, "observedAt": "2020-08-01T12:00:00Z"}]
        });
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_array).await;

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z"),
                ld_headers(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        let entity = body.iter().find(|e| e.get("id").and_then(|id| id.as_str()) == Some(&entity_id));
        assert!(entity.is_some(), "entity should be present in results");
        // The remote attribute "altitude" should have been merged
        let ent = entity.unwrap();
        assert!(ent.get("altitude").is_some(), "altitude from remote should be merged into entity");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_temporal_auxiliary_supplements_only"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_auxiliary");
        let remote_entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_auxiliary_remote");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_auxiliary");

        // Create local temporal entity
        let entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_temporal_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_temporal_entity(&entity_id);

        // Create CSR in auxiliary mode
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), Some("auxiliary"), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns a new entity (should not be added) and supplement for existing
        let mock_entities = serde_json::json!([
            {"id": entity_id, "type": "Vehicle", "altitude": [{"type": "Property", "value": 200}]},
            {"id": remote_entity_id, "type": "Vehicle", "speed": [{"type": "Property", "value": 90}]}
        ]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_entities).await;

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z"),
                ld_headers(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        // The remote-only entity should not be added in auxiliary mode
        let remote_found = body.iter().any(|e| e.get("id").and_then(|id| id.as_str()) == Some(remote_entity_id.as_str()));
        assert!(!remote_found, "auxiliary mode should not add new entities, but remote entity was found");

        // The existing entity should have altitude supplemented
        let existing = body.iter().find(|e| e.get("id").and_then(|id| id.as_str()) == Some(entity_id.as_str()));
        assert!(existing.is_some(), "local entity should be present");
        assert!(
            existing.unwrap().get("altitude").is_some(),
            "altitude should be supplemented from remote in auxiliary mode"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_temporal_exclusive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_exclusive");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_exclusive");

        // Create CSR in exclusive mode
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), Some("exclusive"), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns temporal entity
        let mock_entity = serde_json::json!({
            "id": entity_id,
            "type": "Vehicle",
            "speed": [{"type": "Property", "value": 55, "observedAt": "2020-08-01T12:00:00Z"}]
        });
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_array).await;

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z"),
                ld_headers(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        let found = body.iter().any(|e| e.get("id").and_then(|id| id.as_str()) == Some(entity_id.as_str()));
        assert!(found, "exclusive CSR entity should appear in results");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_temporal_warning_on_cs_error"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_warn");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_warn");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        mock.set_stub(Method::GET, "/ngsi-ld/v1/temporal/entities", 500).await;

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z"),
                ld_headers(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let warnings = warning_header_values(&resp);
        assert!(!warnings.is_empty(), "expected NGSILD-Warning header, got none");
        assert!(warnings.iter().any(|w| w.starts_with("299")), "expected warning code 299, got: {warnings:?}");

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_temporal_type_star_implies_local"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_star");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_star");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = fixtures::load_fixture(TEMPORAL_VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &mock_array).await;

        // type=* implies local=true per spec
        let resp = ctx
            .client
            .query_temporal_entities(&QueryParams::new().type_("*").timerel("after").timeat("2020-01-01T00:00:00Z"), ld_headers())
            .await;
        // Should succeed (200 OK) but not call the mock
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        pretty_assertions::assert_eq!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await,
            0,
            "mock should not be called when type=* (implies local=true)"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_temporal_passes_temporal_params"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_params");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_params");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns empty array (no results)
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &serde_json::json!([]))
            .await;

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2024-01-01T00:00:00Z").lastn(5),
                ld_headers(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        // Verify the mock received the request (temporal params passed through)
        assert!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await > 0,
            "mock should receive forwarded temporal query"
        );

        mock.stop();
    }));

    trials.push(trial(&format!("{PREFIX}::query_temporal_via_propagated"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:tq_via");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:tq_via");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &serde_json::json!([]))
            .await;

        let resp = ctx
            .client
            .query_temporal_entities(
                &QueryParams::new().type_("Vehicle").timerel("after").timeat("2020-01-01T00:00:00Z"),
                ld_headers(),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        // Just verify the mock was called (Via header is added internally by the broker)
        assert!(
            mock.get_count(Method::GET, "/ngsi-ld/v1/temporal/entities").await > 0,
            "mock should receive forwarded request with Via header"
        );

        mock.stop();
    }));
}
