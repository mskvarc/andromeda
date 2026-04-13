#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{client::RequestHeaders, fixtures, mock_server::MockServer, trial::trial};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

use super::super::entities::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entity_attributes::partial_update_attr";

const VEHICLE_DIFFERENT_FIXTURE: &str = "entities/vehicle-simple-different-attributes.jsonld";
const FRAGMENT_SPEED_DATASETID: &str = "entities/fragment_entities/vehicle-speed-two-datasetid-01-fragment.jsonld";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D005_01_inc — Partial attribute update with inclusive registration
    trials.push(trial(
        &format!("{PREFIX}::D005_01_inc::partial_update_with_inclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D005_01_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D005_01_inc");

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create inclusive registration
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), Some("inclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded partial update
            let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::PATCH, &stub_path, 204).await;

            // Partial update attribute
            let fragment = fixtures::load_fragment(FRAGMENT_SPEED_DATASETID);
            let resp = ctx.client.partial_update_attr(&entity_id, "speed", &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify forwarding happened
            assert!(mock.get_count(Method::PATCH, &stub_path).await > 0);

            // Verify local entity contains both isParked2 and speed
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
            assert!(body.get("isParked2").is_some(), "isParked2 should exist");
            assert!(body.get("speed").is_some(), "speed should exist");

            mock.stop();
        },
    ));

    // D005_01_exc — Partial attribute update with exclusive registration
    trials.push(trial(
        &format!("{PREFIX}::D005_01_exc::partial_update_with_exclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D005_01_exc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D005_01_exc");

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Create exclusive registration
            let reg = prepare_registration(REG_SPEED_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded partial update
            let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs/speed");
            mock.set_stub(Method::PATCH, &stub_path, 204).await;

            // Partial update attribute
            let fragment = fixtures::load_fragment(FRAGMENT_SPEED_DATASETID);
            let resp = ctx.client.partial_update_attr(&entity_id, "speed", &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify forwarding happened
            assert!(mock.get_count(Method::PATCH, &stub_path).await > 0);

            // Verify local entity does NOT contain speed (exclusive), isParked2 unchanged
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
            assert!(body.get("speed").is_none(), "speed should not be in local entity");
            assert!(body.get("isParked2").is_some(), "isParked2 should be unchanged");

            mock.stop();
        },
    ));

    // D005_01_red — Partial attribute update with redirect registrations
    trials.push(trial(
        &format!("{PREFIX}::D005_01_red::partial_update_with_redirect_registrations"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D005_01_red");
            let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D005_01_red_1");
            let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D005_01_red_2");

            // Create two redirect registrations for same entity
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
            mock.set_stub(Method::PATCH, &stub_path1, 204).await;
            mock.set_stub(Method::PATCH, &stub_path2, 204).await;

            // Partial update attribute
            let fragment = fixtures::load_fragment(FRAGMENT_SPEED_DATASETID);
            let resp = ctx.client.partial_update_attr(&entity_id, "speed", &fragment).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify both stubs called
            assert!(mock.get_count(Method::PATCH, &stub_path1).await > 0);
            assert!(mock.get_count(Method::PATCH, &stub_path2).await > 0);

            mock.stop();
        },
    ));
}
