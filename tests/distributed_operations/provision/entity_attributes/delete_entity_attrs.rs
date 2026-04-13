#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    mock_server::MockServer,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

use super::super::entities::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entity_attributes::delete_entity_attrs";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.jsonld";
const VEHICLE_DELETE_FIXTURE: &str = "entities/expectations/vehicle-delete-two-datasetid-speed.jsonld";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D006_01_inc — Delete entity attribute (default instance) with inclusive registration
    trials.push(trial(
        &format!("{PREFIX}::D006_01_inc::delete_attr_with_inclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D006_01_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D006_01_inc");

            // Create entity with two datasetId speed attributes
            let entity = fixtures::load_fixture(VEHICLE_DELETE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create inclusive registration
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), Some("inclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded delete
            let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::DELETE, &stub_path, 204).await;

            // Delete attribute (default instance, no deleteAll)
            let resp = ctx.client.delete_entity_attr(&entity_id, "speed").await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify local entity still contains speed (only default instance deleted)
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    &format!("/entities/{entity_id}?local=true"),
                    None,
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            let body: serde_json::Value = resp.json();
            assert!(body.get("speed").is_some(), "speed should still exist (other instances remain)");

            mock.stop();
        },
    ));

    // D006_01_exc — Delete entity attribute (default instance) with exclusive registration
    trials.push(trial(
        &format!("{PREFIX}::D006_01_exc::delete_attr_with_exclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D006_01_exc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D006_01_exc");

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
            let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::DELETE, &stub_path, 204).await;

            // Delete attribute
            let resp = ctx.client.delete_entity_attr(&entity_id, "speed").await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify forwarding happened
            assert!(mock.get_count(Method::DELETE, &stub_path).await > 0);

            mock.stop();
        },
    ));

    // D006_01_red — Delete entity attribute (default instance) with redirect registrations
    trials.push(trial(
        &format!("{PREFIX}::D006_01_red::delete_attr_with_redirect_registrations"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D006_01_red");
            let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D006_01_red_1");
            let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D006_01_red_2");

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_DELETE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create two redirect registrations
            let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id1, &entity_id, &mock.url("/broker1"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id1);

            let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id, &mock.url("/broker2"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id2);

            // Set stubs for both brokers
            let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::DELETE, &stub_path1, 204).await;
            mock.set_stub(Method::DELETE, &stub_path2, 204).await;

            // Delete attribute
            let resp = ctx.client.delete_entity_attr(&entity_id, "speed").await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify both stubs called
            assert!(mock.get_count(Method::DELETE, &stub_path1).await > 0);
            assert!(mock.get_count(Method::DELETE, &stub_path2).await > 0);

            mock.stop();
        },
    ));

    // D006_02_inc — Delete entity attribute (deleteAll=true) with inclusive registration
    trials.push(trial(
        &format!("{PREFIX}::D006_02_inc::delete_attr_deleteall_with_inclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D006_02_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D006_02_inc");

            // Create entity with two datasetId speed attributes
            let entity = fixtures::load_fixture(VEHICLE_DELETE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create inclusive registration
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), Some("inclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded delete
            let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::DELETE, &stub_path, 204).await;

            // Delete attribute with deleteAll=true
            let resp = ctx
                .client
                .delete_entity_attr_with_params(&entity_id, "speed", &QueryParams::new().delete_all(true))
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify local entity does NOT contain speed (all instances deleted)
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    &format!("/entities/{entity_id}?local=true"),
                    None,
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            let body: serde_json::Value = resp.json();
            assert!(body.get("speed").is_none(), "speed should be fully deleted");

            mock.stop();
        },
    ));

    // D006_02_exc — Delete entity attribute (deleteAll=true) with exclusive registration
    trials.push(trial(
        &format!("{PREFIX}::D006_02_exc::delete_attr_deleteall_with_exclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D006_02_exc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D006_02_exc");

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
            let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::DELETE, &stub_path, 204).await;

            // Delete attribute with deleteAll=true
            let resp = ctx
                .client
                .delete_entity_attr_with_params(&entity_id, "speed", &QueryParams::new().delete_all(true))
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify forwarding happened
            assert!(mock.get_count(Method::DELETE, &stub_path).await > 0);

            mock.stop();
        },
    ));

    // D006_02_red — Delete entity attribute (deleteAll=true) with redirect registrations
    trials.push(trial(
        &format!("{PREFIX}::D006_02_red::delete_attr_deleteall_with_redirect_registrations"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D006_02_red");
            let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D006_02_red_1");
            let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D006_02_red_2");

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_DELETE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create two redirect registrations
            let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id1, &entity_id, &mock.url("/broker1"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id1);

            let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id, &mock.url("/broker2"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id2);

            // Set stubs for both brokers
            let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::DELETE, &stub_path1, 204).await;
            mock.set_stub(Method::DELETE, &stub_path2, 204).await;

            // Delete attribute with deleteAll=true
            let resp = ctx
                .client
                .delete_entity_attr_with_params(&entity_id, "speed", &QueryParams::new().delete_all(true))
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify both stubs called
            assert!(mock.get_count(Method::DELETE, &stub_path1).await > 0);
            assert!(mock.get_count(Method::DELETE, &stub_path2).await > 0);

            mock.stop();
        },
    ));
}
