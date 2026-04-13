#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{client::RequestHeaders, fixtures, mock_server::MockServer, trial::trial};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

use super::super::entities::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entity_attributes::append_entity_attrs";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.jsonld";
const FRAGMENT_SPEED_ISPARKED: &str = "entities/fragment_entities/vehicle-speed-isParked-fragment.json";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D003_01_inc — Append entity attributes with inclusive registration
    trials.push(trial(&format!("{PREFIX}::D003_01_inc::append_with_inclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D003_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D003_01_inc");

        // Create entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Get original isParked value
        let resp = ctx
            .client
            .get_entity(
                &entity_id,
                &Default::default(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        let old_body: serde_json::Value = resp.json();
        let old_is_parked = old_body["isParked"].clone();

        // Create inclusive registration
        let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Set stub for forwarded append
        let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
        mock.set_stub(Method::POST, &stub_path, 204).await;

        // Append attributes
        let fragment = fixtures::load_fragment(FRAGMENT_SPEED_ISPARKED);
        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{entity_id}/attrs"),
                Some(&fragment),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding happened
        pretty_assertions::assert_eq!(mock.get_count(Method::POST, &stub_path).await, 1);

        // Verify local entity has speed and isParked changed
        let resp = ctx
            .client
            .get_entity(
                &entity_id,
                &Default::default(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: serde_json::Value = resp.json();
        assert!(body.get("speed").is_some(), "speed attribute should exist");
        pretty_assertions::assert_ne!(body["isParked"], old_is_parked, "isParked should have changed");

        mock.stop();
    }));

    // D003_01_exc — Append entity attributes with exclusive registration
    trials.push(trial(&format!("{PREFIX}::D003_01_exc::append_with_exclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D003_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D003_01_exc");

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

        // Set stub for forwarded append
        let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
        mock.set_stub(Method::POST, &stub_path, 204).await;

        // Append attributes
        let fragment = fixtures::load_fragment(FRAGMENT_SPEED_ISPARKED);
        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{entity_id}/attrs"),
                Some(&fragment),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding happened
        pretty_assertions::assert_eq!(mock.get_count(Method::POST, &stub_path).await, 1);

        // Verify local entity does NOT contain speed (exclusive)
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

        mock.stop();
    }));

    // D003_01_red — Append entity attributes with redirect registrations
    trials.push(trial(&format!("{PREFIX}::D003_01_red::append_with_redirect_registrations"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D003_01_red");
        let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D003_01_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D003_01_red_2");

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
        let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
        let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs");
        mock.set_stub(Method::POST, &stub_path1, 204).await;
        mock.set_stub(Method::POST, &stub_path2, 204).await;

        // Append attributes
        let fragment = fixtures::load_fragment(FRAGMENT_SPEED_ISPARKED);
        let resp = ctx
            .client
            .request(
                Method::POST,
                &format!("/entities/{entity_id}/attrs"),
                Some(&fragment),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify both stubs called
        pretty_assertions::assert_eq!(mock.get_count(Method::POST, &stub_path1).await, 1);
        pretty_assertions::assert_eq!(mock.get_count(Method::POST, &stub_path2).await, 1);

        // Verify entity does not exist locally
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
    }));

    // D003_02_inc — Append entity attributes with noOverwrite, inclusive
    trials.push(trial(
        &format!("{PREFIX}::D003_02_inc::append_nooverwrite_with_inclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D003_02_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D003_02_inc");

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Get original isParked value
            let resp = ctx
                .client
                .get_entity(
                    &entity_id,
                    &Default::default(),
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            let old_body: serde_json::Value = resp.json();
            let old_is_parked = old_body["isParked"].clone();

            // Create inclusive registration
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), Some("inclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded append (207 for noOverwrite partial success)
            let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
            mock.set_stub(Method::POST, &stub_path, 207).await;

            // Append attributes with noOverwrite
            let fragment = fixtures::load_fragment(FRAGMENT_SPEED_ISPARKED);
            let resp = ctx
                .client
                .request(
                    Method::POST,
                    &format!("/entities/{entity_id}/attrs?options=noOverwrite"),
                    Some(&fragment),
                    RequestHeaders::new()
                        .content_type("application/json")
                        .link_context(fixtures::test_suite_context_url()),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

            // Verify forwarding happened
            assert!(mock.get_count(Method::POST, &stub_path).await > 0);

            // Verify speed exists but isParked unchanged
            let resp = ctx
                .client
                .get_entity(
                    &entity_id,
                    &Default::default(),
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            let body: serde_json::Value = resp.json();
            assert!(body.get("speed").is_some(), "speed should exist");
            pretty_assertions::assert_eq!(body["isParked"], old_is_parked, "isParked should be unchanged");

            mock.stop();
        },
    ));

    // D003_02_exc — Append entity attributes with noOverwrite, exclusive
    trials.push(trial(
        &format!("{PREFIX}::D003_02_exc::append_nooverwrite_with_exclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D003_02_exc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D003_02_exc");

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

            // Set stub (207 for noOverwrite)
            let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
            mock.set_stub(Method::POST, &stub_path, 207).await;

            // Append attributes with noOverwrite
            let fragment = fixtures::load_fragment(FRAGMENT_SPEED_ISPARKED);
            let resp = ctx
                .client
                .request(
                    Method::POST,
                    &format!("/entities/{entity_id}/attrs?options=noOverwrite"),
                    Some(&fragment),
                    RequestHeaders::new()
                        .content_type("application/json")
                        .link_context(fixtures::test_suite_context_url()),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

            // Verify forwarding happened
            assert!(mock.get_count(Method::POST, &stub_path).await > 0);

            mock.stop();
        },
    ));

    // D003_02_red — Append entity attributes with noOverwrite, redirect
    trials.push(trial(
        &format!("{PREFIX}::D003_02_red::append_nooverwrite_with_redirect_registrations"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D003_02_red");
            let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D003_02_red_1");
            let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D003_02_red_2");

            // Create two redirect registrations
            let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id1, &entity_id, &mock.url("/broker1"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id1);

            let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id, &mock.url("/broker2"), Some("redirect"));
            let resp = ctx.client.create_registration(&reg2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id2);

            // Set stubs (207 for noOverwrite)
            let stub_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
            let stub_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}/attrs");
            mock.set_stub(Method::POST, &stub_path1, 207).await;
            mock.set_stub(Method::POST, &stub_path2, 207).await;

            // Append attributes with noOverwrite
            let fragment = fixtures::load_fragment(FRAGMENT_SPEED_ISPARKED);
            let resp = ctx
                .client
                .request(
                    Method::POST,
                    &format!("/entities/{entity_id}/attrs?options=noOverwrite"),
                    Some(&fragment),
                    RequestHeaders::new()
                        .content_type("application/json")
                        .link_context(fixtures::test_suite_context_url()),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

            // Verify both stubs called
            assert!(mock.get_count(Method::POST, &stub_path1).await > 0);
            assert!(mock.get_count(Method::POST, &stub_path2).await > 0);

            mock.stop();
        },
    ));
}
