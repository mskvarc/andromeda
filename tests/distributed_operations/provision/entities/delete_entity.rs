#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{client::RequestHeaders, fixtures, mock_server::MockServer, trial::trial};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

use super::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entities::delete_entity";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.jsonld";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";
const REG_SPEED_DEFAULT_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-default-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D002_01_inc — Delete Entities On Both Context Broker And Context Source
    trials.push(trial(
        &format!("{PREFIX}::D002_01_inc::delete_on_both_broker_and_context_source"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_01_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D002_01_inc");

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create inclusive registration
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded delete
            let delete_path = format!("/ngsi-ld/v1/entities/{entity_id}");
            mock.set_stub(Method::DELETE, &delete_path, 204).await;

            // Delete entity
            let resp = ctx.client.delete_entity(&entity_id).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify forwarding happened
            assert!(mock.get_count(Method::DELETE, &delete_path).await > 0);

            // Verify entity is gone locally
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    &format!("/entities/{entity_id}?local=true"),
                    None,
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);

            mock.stop();
        },
    ));

    // D002_01_exc — Delete Entity With Exclusive Registration
    trials.push(trial(&format!("{PREFIX}::D002_01_exc::delete_with_exclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D002_01_exc");

        // Create entity locally
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive registration
        let reg = prepare_registration(REG_SPEED_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Set stub for forwarded delete
        let delete_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
        mock.set_stub(Method::DELETE, &delete_path, 204).await;

        // Delete entity
        let resp = ctx.client.delete_entity(&entity_id).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding happened
        assert!(mock.get_count(Method::DELETE, &delete_path).await > 0);

        mock.stop();
    }));

    // D002_01_red — Delete Entities On Both Context Sources
    trials.push(trial(&format!("{PREFIX}::D002_01_red::delete_on_both_context_sources"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_01_red_1");
        let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_01_red_2");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D002_01_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D002_01_red_2");

        // Create two redirect registrations
        let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg1).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id2, &mock.url("/broker2"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id2);

        // Create entities via redirect
        mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 201).await;
        mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entities", 201).await;

        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        let entity2 = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id2);
        let resp = ctx.client.create_entity(&entity2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Set stubs for forwarded deletes
        let delete_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
        let delete_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id2}");
        mock.set_stub(Method::DELETE, &delete_path1, 204).await;
        mock.set_stub(Method::DELETE, &delete_path2, 204).await;

        // Delete entities
        let resp = ctx.client.delete_entity(&entity_id).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);
        let resp = ctx.client.delete_entity(&entity_id2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding happened for both
        assert!(mock.get_count(Method::DELETE, &delete_path1).await > 0);
        assert!(mock.get_count(Method::DELETE, &delete_path2).await > 0);

        mock.stop();
    }));

    // D002_02_01_inc — Delete Entity On The Context Broker (remote fails)
    trials.push(trial(
        &format!("{PREFIX}::D002_02_01_inc::delete_local_success_remote_failure"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_02_01_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D002_02_01_inc");

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create inclusive registration
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for remote (not found)
            let delete_path = format!("/ngsi-ld/v1/entities/{entity_id}");
            mock.set_stub(Method::DELETE, &delete_path, 404).await;

            // Delete entity → 207 (local success, remote 404)
            let resp = ctx.client.delete_entity(&entity_id).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

            let body: serde_json::Value = resp.json();
            pretty_assertions::assert_eq!(body["errors"][0]["error"]["status"], 404);

            // Verify entity is gone locally
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    &format!("/entities/{entity_id}?local=true"),
                    None,
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);

            mock.stop();
        },
    ));

    // D002_02_02_inc — Delete Entity On A Context Source (local fails)
    trials.push(trial(
        &format!("{PREFIX}::D002_02_02_inc::delete_local_failure_remote_success"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_02_02_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D002_02_02_inc");

            // Create inclusive registration (no local entity)
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for remote (success)
            let delete_path = format!("/ngsi-ld/v1/entities/{entity_id}");
            mock.set_stub(Method::DELETE, &delete_path, 204).await;

            // Delete entity → 207 (local 404, remote success)
            let resp = ctx.client.delete_entity(&entity_id).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

            let body: serde_json::Value = resp.json();
            let errors = body["errors"].as_array().unwrap();
            pretty_assertions::assert_eq!(errors.len(), 1);
            pretty_assertions::assert_eq!(errors[0]["error"]["status"], 404);

            mock.stop();
        },
    ));

    // D002_02_exc — Delete Entity Without Redirection Operations
    trials.push(trial(
        &format!("{PREFIX}::D002_02_exc::delete_without_redirection_operations"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_02_exc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D002_02_exc");

            // Create entity locally
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let resp = ctx
                .client
                .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create exclusive registration WITHOUT redirectionOps
            let reg = prepare_registration(REG_SPEED_DEFAULT_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub (should never be called)
            let delete_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
            mock.set_stub(Method::DELETE, &delete_path, 207).await;

            // Delete entity
            let resp = ctx.client.delete_entity(&entity_id).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

            // Verify forwarding did NOT happen
            pretty_assertions::assert_eq!(mock.get_count(Method::DELETE, &delete_path).await, 0);

            mock.stop();
        },
    ));

    // D002_03_inc — Delete Entity With Local Flag
    trials.push(trial(&format!("{PREFIX}::D002_03_inc::delete_with_local_flag"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D002_03_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D002_03_inc");

        // Create entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create inclusive registration
        let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Delete entity with local=true → should NOT forward
        let resp = ctx
            .client
            .request(Method::DELETE, &format!("/entities/{entity_id}?local=true"), None, RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify no forwarding happened
        pretty_assertions::assert_eq!(mock.get_total_count().await, 0);

        mock.stop();
    }));
}
