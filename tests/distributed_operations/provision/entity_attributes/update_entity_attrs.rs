#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{client::RequestHeaders, fixtures, mock_server::MockServer, trial::trial};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

use super::super::entities::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entity_attributes::update_entity_attrs";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.jsonld";
const FRAGMENT_SPEED_DIFFERENT: &str = "entities/fragment_entities/vehicle-speed-different-attribute.jsonld";
const FRAGMENT_BRANDNAME: &str = "entities/fragment_entities/vehicle-brandname-complete-fragment.jsonld";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D004_01_exc — Update entity attributes with exclusive registration
    trials.push(trial(&format!("{PREFIX}::D004_01_exc::update_with_exclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D004_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D004_01_exc");

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

        // Set stub for forwarded update
        let stub_path = format!("/broker1/ngsi-ld/v1/entities/{entity_id}/attrs");
        mock.set_stub(Method::PATCH, &stub_path, 204).await;

        // Update entity attributes
        let fragment = fixtures::load_fragment(FRAGMENT_SPEED_DIFFERENT);
        let resp = ctx.client.update_entity_attrs(&entity_id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify forwarding happened
        assert!(mock.get_count(Method::PATCH, &stub_path).await > 0);

        mock.stop();
    }));

    // D004_01_inc — Update entity attributes with inclusive registration
    trials.push(trial(&format!("{PREFIX}::D004_01_inc::update_with_inclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D004_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D004_01_inc");

        // No local entity — only registration
        let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Set stub for forwarded update
        let stub_path = format!("/ngsi-ld/v1/entities/{entity_id}/attrs");
        mock.set_stub(Method::PATCH, &stub_path, 204).await;

        // Update entity attributes — local fails (no entity), remote succeeds → 207
        let fragment = fixtures::load_fragment(FRAGMENT_BRANDNAME);
        let resp = ctx.client.update_entity_attrs(&entity_id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

        // Verify forwarding happened
        assert!(mock.get_count(Method::PATCH, &stub_path).await > 0);

        mock.stop();
    }));

    // D004_01_red — Update entity attributes with redirect registrations
    trials.push(trial(&format!("{PREFIX}::D004_01_red::update_with_redirect_registrations"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D004_01_red");
        let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D004_01_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D004_01_red_2");

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
        mock.set_stub(Method::PATCH, &stub_path1, 204).await;
        mock.set_stub(Method::PATCH, &stub_path2, 204).await;

        // Update entity attributes
        let fragment = fixtures::load_fragment(FRAGMENT_BRANDNAME);
        let resp = ctx.client.update_entity_attrs(&entity_id, &fragment).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NO_CONTENT);

        // Verify both stubs called
        assert!(mock.get_count(Method::PATCH, &stub_path1).await > 0);
        assert!(mock.get_count(Method::PATCH, &stub_path2).await > 0);

        mock.stop();
    }));
}
