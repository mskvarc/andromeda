use crate::harness::{client::RequestHeaders, fixtures, mock_server::MockServer, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

use super::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entities::merge_entity";

const VEHICLE_SECOND_FIXTURE: &str = "entities/vehicle-simple-attributes-second.jsonld";
const VEHICLE_SECOND_DIFFERENT: &str = "entities/vehicle-simple-attributes-second-different.jsonld";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D008_01_exc — Merge Entity (exclusive)
    trials.push(trial(&format!("{PREFIX}::D008_01_exc::merge_with_exclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D008_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D008_01_exc");

        // Create entity locally
        let entity = fixtures::load_fixture(VEHICLE_SECOND_FIXTURE, &entity_id);
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

        // Set stub for forwarded merge
        let patch_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
        mock.set_stub(Method::PATCH, &patch_path, 204).await;

        // Merge entity
        let merge_payload = fixtures::load_fixture(VEHICLE_SECOND_DIFFERENT, &entity_id);
        let resp = ctx.client.merge_entity(&entity_id, &merge_payload).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding happened
        assert!(mock.get_count(Method::PATCH, &patch_path).await > 0);

        // Verify local entity does not contain speed
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
    }));

    // D008_01_inc — Merge Entity (inclusive)
    trials.push(trial(&format!("{PREFIX}::D008_01_inc::merge_with_inclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D008_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D008_01_inc");

        // Create entity
        let entity = fixtures::load_fixture(VEHICLE_SECOND_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
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
        let old_isparked2 = old_body["isParked2"].clone();
        let old_brandname = old_body["brandName"].clone();

        // Set stub for forwarded merge
        let patch_path = format!("/ngsi-ld/v1/entities/{entity_id}");
        mock.set_stub(Method::PATCH, &patch_path, 204).await;

        // Merge entity
        let merge_payload = fixtures::load_fixture(VEHICLE_SECOND_DIFFERENT, &entity_id);
        let resp = ctx.client.merge_entity(&entity_id, &merge_payload).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding happened
        assert!(mock.get_count(Method::PATCH, &patch_path).await > 0);

        // Verify entity was merged correctly
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
        assert_eq!(old_brandname, new_body["brandName"]); // brandName unchanged
        assert_ne!(old_isparked2, new_body["isParked2"]); // isParked2 changed
        assert!(new_body.as_object().unwrap().contains_key("speed")); // speed added

        mock.stop();
    }));

    // D008_01_red — Merge Entity (redirect)
    trials.push(trial(&format!("{PREFIX}::D008_01_red::merge_with_redirect_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D008_01_red");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D008_01_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D008_01_red_2");

        // Create two redirect registrations (same entity_id)
        let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg1).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id, &mock.url("/broker2"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg2).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id2);

        // Set stubs for forwarded merges
        let patch_path1 = format!("/broker1/ngsi-ld/v1/entities/{entity_id}");
        let patch_path2 = format!("/broker2/ngsi-ld/v1/entities/{entity_id}");
        mock.set_stub(Method::PATCH, &patch_path1, 204).await;
        mock.set_stub(Method::PATCH, &patch_path2, 204).await;

        // Merge entity
        let merge_payload = fixtures::load_fixture(VEHICLE_SECOND_DIFFERENT, &entity_id);
        let resp = ctx.client.merge_entity(&entity_id, &merge_payload).await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding to both
        assert!(mock.get_count(Method::PATCH, &patch_path1).await > 0);
        assert!(mock.get_count(Method::PATCH, &patch_path2).await > 0);

        mock.stop();
    }));
}
