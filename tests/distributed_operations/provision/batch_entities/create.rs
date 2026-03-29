use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    mock_server::MockServer,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

use super::super::entities::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::batch_entities::create";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.jsonld";
const VEHICLE_DIFFERENT_FIXTURE: &str = "entities/vehicle-simple-different-attributes.jsonld";
const VEHICLE_SPEED_FIXTURE: &str = "entities/vehicle-speed-attribute.jsonld";
const REG_BATCH_OPS: &str = "csource_registrations/context-source-registration-vehicle-batch-ops.jsonld";
const REG_SPEED_BATCH_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-batch-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D012_01_exc — Batch Create Entities With Exclusive Registration
    trials.push(trial(
        &format!("{PREFIX}::D012_01_exc::batch_create_with_exclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:D012_01_exc_1");
            let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D012_01_exc_2");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D012_01_exc");

            // Create 2 entities locally first (before CSR exists)
            let e1 = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id1);
            let resp = ctx.client.create_entity(&e1).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let e2 = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id2);
            let resp = ctx.client.create_entity(&e2).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Create exclusive CSR with propertyNames=["speed"]
            let reg = prepare_registration(REG_SPEED_BATCH_OPS, &reg_id, "", &mock.url("/broker1"), Some("exclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Stub forwarded batch create
            mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create", 201).await;

            // Batch create speed entities
            let speed1 = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id1);
            let speed2 = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id2);
            let batch = serde_json::json!([speed1, speed2]);
            let resp = ctx.client.batch_create(&batch).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Verify forwarding happened
            assert_eq!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create").await, 1);

            mock.stop();
        },
    ));

    // D012_01_inc — Batch Create Entities With Inclusive Registration
    trials.push(trial(
        &format!("{PREFIX}::D012_01_inc::batch_create_with_inclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:D012_01_inc_1");
            let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D012_01_inc_2");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D012_01_inc");

            // Create inclusive CSR
            let reg = prepare_registration(REG_BATCH_OPS, &reg_id, "", &mock.url(""), None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Stub forwarded batch create
            mock.set_stub(Method::POST, "/ngsi-ld/v1/entityOperations/create", 201).await;

            // Batch create entities
            let e1 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id1);
            let e2 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id2);
            let batch = serde_json::json!([e1, e2]);
            let resp = ctx.client.batch_create(&batch).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Verify forwarding happened
            assert_eq!(mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/create").await, 1);

            // Verify entities exist locally
            let resp = ctx
                .client
                .query_entities(
                    &QueryParams::new().type_("Vehicle"),
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(body.iter().any(|e| e["id"] == entity_id1));
            assert!(body.iter().any(|e| e["id"] == entity_id2));

            mock.stop();
        },
    ));

    // D012_01_red — Batch Create Entities With Redirect Registration
    trials.push(trial(
        &format!("{PREFIX}::D012_01_red::batch_create_with_redirect_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:D012_01_red_1");
            let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D012_01_red_2");
            let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D012_01_red_1");
            let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D012_01_red_2");

            // Create 2 redirect CSRs
            let reg1 = prepare_registration(REG_BATCH_OPS, &reg_id1, "", &mock.url("/broker1"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg1).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id1);

            let reg2 = prepare_registration(REG_BATCH_OPS, &reg_id2, "", &mock.url("/broker2"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg2).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id2);

            // Stub both brokers
            mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create", 201).await;
            mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/create", 201).await;

            // Batch create entities
            let e1 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id1);
            let e2 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id2);
            let batch = serde_json::json!([e1, e2]);
            let resp = ctx.client.batch_create(&batch).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Verify forwarding to both brokers
            assert_eq!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/create").await, 1);
            assert_eq!(mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/create").await, 1);

            mock.stop();
        },
    ));
}
