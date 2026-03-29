use crate::harness::{client::RequestHeaders, fixtures, mock_server::MockServer, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

use super::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entities::replace_entity";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.jsonld";
const VEHICLE_JSON_FIXTURE: &str = "entities/vehicle-simple-attributes.json";
const VEHICLE_REPLACEMENT: &str = "entities/vehicle-simple-different-attributes.json";
const VEHICLE_SECOND: &str = "entities/vehicle-simple-attributes-second.json";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D007_01_exc — Replace Entity (exclusive)
    trials.push(trial(
        &format!("{PREFIX}::D007_01_exc::replace_with_exclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D007_01_exc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D007_01_exc");

            // Create entity locally
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let resp = ctx
                .client
                .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Create exclusive registration
            let reg = prepare_registration(REG_SPEED_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded replace
            let put_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
            mock.set_stub(Method::PUT, &put_path, 204).await;

            // Replace entity
            let replacement = fixtures::load_fixture(VEHICLE_REPLACEMENT, &entity_id);
            let resp = ctx
                .client
                .request(
                    Method::PUT,
                    &format!("/entities/{entity_id}"),
                    Some(&replacement),
                    RequestHeaders::new()
                        .content_type("application/json")
                        .link_context(fixtures::test_suite_context_url()),
                )
                .await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify forwarding happened
            assert!(mock.get_count(Method::PUT, &put_path).await > 0);

            // Per spec 4.3.6.3: exclusive attributes shall not be held locally
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
            assert!(!body.as_object().unwrap().contains_key("speed"));

            mock.stop();
        },
    ));

    // D007_01_inc — Replace Entity (inclusive)
    trials.push(trial(
        &format!("{PREFIX}::D007_01_inc::replace_with_inclusive_registration"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D007_01_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D007_01_inc");

            // Create entity with JSON content type + link context
            let entity = fixtures::load_fixture(VEHICLE_JSON_FIXTURE, &entity_id);
            let resp = ctx
                .client
                .request(
                    Method::POST,
                    "/entities",
                    Some(&entity),
                    RequestHeaders::new()
                        .content_type("application/json")
                        .link_context(fixtures::test_suite_context_url()),
                )
                .await;
            assert_eq!(resp.status, StatusCode::CREATED);

            // Create inclusive registration
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), Some("inclusive"));
            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Get old entity body
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    &format!("/entities/{entity_id}"),
                    None,
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            let old_body: serde_json::Value = resp.json();

            // Set stub for forwarded replace
            let put_path = format!("/ngsi-ld/v1/entities/{entity_id}");
            mock.set_stub(Method::PUT, &put_path, 204).await;

            // Replace entity
            let replacement = fixtures::load_fixture(VEHICLE_SECOND, &entity_id);
            let resp = ctx
                .client
                .request(
                    Method::PUT,
                    &format!("/entities/{entity_id}"),
                    Some(&replacement),
                    RequestHeaders::new()
                        .content_type("application/json")
                        .link_context(fixtures::test_suite_context_url()),
                )
                .await;
            assert_eq!(resp.status, StatusCode::NO_CONTENT);

            // Verify entity body changed
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    &format!("/entities/{entity_id}"),
                    None,
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            let new_body: serde_json::Value = resp.json();
            assert_ne!(old_body, new_body);

            mock.stop();
        },
    ));

    // D007_01_red — Replace Entity (redirect)
    trials.push(trial(&format!("{PREFIX}::D007_01_red::replace_with_redirect_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D007_01_red");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D007_01_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D007_01_red_2");

        // Create two redirect registrations (same entity_id for both)
        let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id, &mock.url("/broker2"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id2);

        // Set stubs for forwarded replaces
        let put_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
        let put_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}");
        mock.set_stub(Method::PUT, &put_path1, 204).await;
        mock.set_stub(Method::PUT, &put_path2, 204).await;

        // Replace entity
        let replacement = fixtures::load_fixture(VEHICLE_SECOND, &entity_id);
        let resp = ctx
            .client
            .request(
                Method::PUT,
                &format!("/entities/{entity_id}"),
                Some(&replacement),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding to both
        assert_eq!(mock.get_count(Method::PUT, &put_path1).await, 1);
        assert_eq!(mock.get_count(Method::PUT, &put_path2).await, 1);

        mock.stop();
    }));
}
