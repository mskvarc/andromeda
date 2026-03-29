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

const PREFIX: &str = "distributed_operations::provision::batch_entities::delete";

const VEHICLE_DIFFERENT_FIXTURE: &str = "entities/vehicle-simple-different-attributes.jsonld";
const REG_BATCH_OPS: &str = "csource_registrations/context-source-registration-vehicle-batch-ops.jsonld";
const REG_SPEED_BATCH_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-batch-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D015_01_inc (DeleteBatchEntities) — Delete Batch With Inclusive Registration
    trials.push(trial(&format!("{PREFIX}::D015_01_inc_a::batch_delete_with_inclusive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_inc_a_1");
        let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_inc_a_2");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D015_01_inc_a");

        // Create 2 entities locally
        let e1 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id1);
        let resp = ctx.client.create_entity(&e1).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let e2 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id2);
        let resp = ctx.client.create_entity(&e2).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create inclusive CSR
        let reg = prepare_registration(REG_BATCH_OPS, &reg_id, "", &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Stub forwarded batch delete
        mock.set_stub(Method::POST, "/ngsi-ld/v1/entityOperations/delete", 204).await;

        // Batch delete
        let ids = serde_json::json!([entity_id1, entity_id2]);
        let resp = ctx.client.batch_delete(&ids).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        assert!(resp.body_bytes().is_empty());

        // Verify entities are gone locally
        let resp = ctx
            .client
            .query_entities(
                &QueryParams::new().type_("Vehicle"),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();
        assert!(!body.iter().any(|e| e["id"] == entity_id1));
        assert!(!body.iter().any(|e| e["id"] == entity_id2));

        mock.stop();
    }));

    // D015_01_exc (DeleteBatchOfEntities) — Delete Batch With Exclusive Registration
    trials.push(trial(&format!("{PREFIX}::D015_01_exc::batch_delete_with_exclusive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_exc_1");
        let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_exc_2");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D015_01_exc");

        // Create 2 entities locally
        let e1 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id1);
        let resp = ctx.client.create_entity(&e1).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let e2 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id2);
        let resp = ctx.client.create_entity(&e2).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive CSR with propertyNames=["speed"]
        let reg = prepare_registration(REG_SPEED_BATCH_OPS, &reg_id, "", &mock.url("/broker1"), Some("exclusive"));
        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Stub forwarded batch delete
        mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete", 204).await;

        // Batch delete
        let ids = serde_json::json!([entity_id1, entity_id2]);
        let resp = ctx.client.batch_delete(&ids).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        assert!(resp.body_bytes().is_empty());

        // Verify forwarding happened
        assert_eq!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete").await, 1);

        // Verify entities are gone locally
        let resp = ctx
            .client
            .query_entities(
                &QueryParams::new().type_("Vehicle"),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();
        assert!(!body.iter().any(|e| e["id"] == entity_id1));
        assert!(!body.iter().any(|e| e["id"] == entity_id2));

        mock.stop();
    }));

    // D015_01_inc (DeleteBatchOfEntities) — Delete Batch Entities With Inclusive Registration
    trials.push(trial(
        &format!("{PREFIX}::D015_01_inc_b::batch_delete_entities_with_inclusive"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_inc_b_1");
            let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_inc_b_2");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D015_01_inc_b");

            // Create 2 entities locally
            let e1 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id1);
            let resp = ctx.client.create_entity(&e1).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let e2 = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id2);
            let resp = ctx.client.create_entity(&e2).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Create inclusive CSR
            let reg = prepare_registration(REG_BATCH_OPS, &reg_id, "", &mock.url(""), None);
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Stub forwarded batch delete
            mock.set_stub(Method::POST, "/ngsi-ld/v1/entityOperations/delete", 204).await;

            // Batch delete
            let ids = serde_json::json!([entity_id1, entity_id2]);
            let resp = ctx.client.batch_delete(&ids).await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);
            assert!(resp.body_bytes().is_empty());

            // Verify forwarding happened
            assert_eq!(mock.get_count(Method::POST, "/ngsi-ld/v1/entityOperations/delete").await, 1);

            // Verify entities are gone locally
            let resp = ctx
                .client
                .query_entities(
                    &QueryParams::new().type_("Vehicle"),
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(!body.iter().any(|e| e["id"] == entity_id1));
            assert!(!body.iter().any(|e| e["id"] == entity_id2));

            mock.stop();
        },
    ));

    // D015_01_red (DeleteBatchOfEntities) — Delete Batch With Redirect Registration
    trials.push(trial(&format!("{PREFIX}::D015_01_red::batch_delete_with_redirect"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id1 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_red_1");
        let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D015_01_red_2");
        let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D015_01_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D015_01_red_2");

        // Create 2 redirect CSRs (no local entities for redirect)
        let reg1 = prepare_registration(REG_BATCH_OPS, &reg_id1, "", &mock.url("/broker1"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id1);

        let reg2 = prepare_registration(REG_BATCH_OPS, &reg_id2, "", &mock.url("/broker2"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id2);

        // Stub both brokers
        mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete", 204).await;
        mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/delete", 204).await;

        // Batch delete
        let ids = serde_json::json!([entity_id1, entity_id2]);
        let resp = ctx.client.batch_delete(&ids).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
        assert!(resp.body_bytes().is_empty());

        // Verify forwarding to both brokers
        assert_eq!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entityOperations/delete").await, 1);
        assert_eq!(mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entityOperations/delete").await, 1);

        mock.stop();
    }));
}
