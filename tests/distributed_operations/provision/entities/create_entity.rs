#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    mock_server::MockServer,
    trial::trial,
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

use super::prepare_registration;

const PREFIX: &str = "distributed_operations::provision::entities::create_entity";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.jsonld";
const VEHICLE_SPEED_FIXTURE: &str = "entities/vehicle-speed-attribute.jsonld";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";
const REG_SPEED_DEFAULT_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-default-ops.jsonld";

pub fn tests(trials: &mut Vec<Trial>) {
    // D001_01_inc — Create Entity On Both Context Broker And Context Source
    trials.push(trial(
        &format!("{PREFIX}::D001_01_inc::create_on_both_broker_and_context_source"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_01_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_01_inc");

            // Create inclusive registration (no mode)
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for forwarded create
            mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

            // Create entity
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

            // Verify forwarding happened
            assert!(mock.get_count(Method::POST, "/ngsi-ld/v1/entities").await > 0);

            // Verify entity exists locally
            let resp = ctx
                .client
                .query_entities(
                    &QueryParams::new().type_("Vehicle"),
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
            let body: Vec<serde_json::Value> = resp.json();
            assert!(body.iter().any(|e| e["id"] == entity_id));

            mock.stop();
        },
    ));

    // D001_01_exc — Create Entity With Exclusive Registration
    trials.push(trial(&format!("{PREFIX}::D001_01_exc::create_with_exclusive_registration"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_01_exc");

        // Create entity locally first
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive registration with endpoint /broker1
        let reg = prepare_registration(REG_SPEED_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("exclusive"));
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Set stub for forwarded create
        mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 201).await;

        // Create entity with speed attribute
        let speed_entity = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&speed_entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Verify forwarding happened
        assert!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entities").await > 0);

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

    // D001_01_red — Create Entity On Both Context Sources
    trials.push(trial(&format!("{PREFIX}::D001_01_red::create_on_both_context_sources"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_01_red_1");
        let entity_id2 = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_01_red_2");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_01_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D001_01_red_2");

        // Create two redirect registrations
        let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url("/broker1"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg1).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id2, &mock.url("/broker2"), Some("redirect"));
        let resp = ctx.client.create_registration(&reg2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id2);

        // Set stubs for both brokers
        mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 201).await;
        mock.set_stub(Method::POST, "/broker2/ngsi-ld/v1/entities", 201).await;

        // Create entity1 → forwarded to broker1
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        assert!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entities").await > 0);

        // Create entity2 → forwarded to broker2
        let entity2 = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id2);
        let resp = ctx.client.create_entity(&entity2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        assert!(mock.get_count(Method::POST, "/broker2/ngsi-ld/v1/entities").await > 0);

        mock.stop();
    }));

    // D001_02_exc — Create Entity Without Redirection Operations
    trials.push(trial(
        &format!("{PREFIX}::D001_02_exc::create_without_redirection_operations"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_02_exc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_02_exc");

            // Create entity locally first
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
            mock.set_stub(Method::POST, "/broker1/ngsi-ld/v1/entities", 409).await;

            // Try to create entity with speed → should fail with 409
            let speed_entity = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&speed_entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CONFLICT);

            // Verify forwarding did NOT happen
            pretty_assertions::assert_eq!(mock.get_count(Method::POST, "/broker1/ngsi-ld/v1/entities").await, 0);

            mock.stop();
        },
    ));

    // D001_02_inc — Create Entity With Malformed Id
    trials.push(trial(&format!("{PREFIX}::D001_02_inc::create_with_malformed_id"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_02_inc");

        // Create inclusive registration (no entity_id in this fixture)
        let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, "", &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Try to create entity with invalid URI id
        let mut entity = fixtures::load_fixture(VEHICLE_FIXTURE, "InvalidUriExample");
        entity["id"] = serde_json::Value::String("InvalidUriExample".to_string());
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::BAD_REQUEST);

        // Verify no entity was created locally
        let resp = ctx
            .client
            .query_entities(
                &QueryParams::new().type_("Vehicle"),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);
        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.is_empty());

        mock.stop();
    }));

    // D001_03_01_inc — Create Entity Already Existing Locally
    trials.push(trial(&format!("{PREFIX}::D001_03_01_inc::create_already_existing_locally"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_03_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_03_01_inc");

        // Create entity first (succeeds)
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create inclusive registration
        let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Set stub for remote (succeeds)
        mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 201).await;

        // Create same entity again → 207 (local conflict, remote success)
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

        let body: serde_json::Value = resp.json();
        let errors = body["errors"].as_array().unwrap();
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0]["error"]["status"], 409);

        // Verify forwarding still happened
        assert!(mock.get_count(Method::POST, "/ngsi-ld/v1/entities").await > 0);

        mock.stop();
    }));

    // D001_03_02_inc — Create Entity Already Existing On Both
    trials.push(trial(&format!("{PREFIX}::D001_03_02_inc::create_already_existing_on_both"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_03_02_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_03_02_inc");

        // Create entity locally first
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create inclusive registration
        let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Set stub for remote (conflict — entity exists there too)
        mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 409).await;

        // Create same entity again → 207 (both conflict)
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx.client.create_entity(&entity).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

        let body: serde_json::Value = resp.json();
        let errors = body["errors"].as_array().unwrap();
        pretty_assertions::assert_eq!(errors[0]["error"]["status"], 409);
        pretty_assertions::assert_eq!(errors[1]["error"]["status"], 409);

        mock.stop();
    }));

    // D001_03_03_inc — Create Entity Already Existing Remotely
    trials.push(trial(
        &format!("{PREFIX}::D001_03_03_inc::create_already_existing_remotely"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_03_03_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_03_03_inc");

            // Create inclusive registration (no local entity)
            let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Set stub for remote (conflict — entity exists remotely)
            mock.set_stub(Method::POST, "/ngsi-ld/v1/entities", 409).await;

            // Create entity → 207 (local success, remote conflict)
            let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            let resp = ctx.client.create_entity(&entity).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::MULTI_STATUS);

            let body: serde_json::Value = resp.json();
            let errors = body["errors"].as_array().unwrap();
            pretty_assertions::assert_eq!(errors[0]["error"]["status"], 409);

            // Verify entity was created locally
            let resp = ctx
                .client
                .query_entities(
                    &QueryParams::new().type_("Vehicle"),
                    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
                )
                .await;
            let body: Vec<serde_json::Value> = resp.json();
            assert!(body.iter().any(|e| e["id"] == entity_id));

            mock.stop();
        },
    ));

    // D001_04_inc — Create Entity With Local Flag
    trials.push(trial(&format!("{PREFIX}::D001_04_inc::create_with_local_flag"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D001_04_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D001_04_inc");

        // Create inclusive registration
        let reg = prepare_registration(REG_REDIRECTION_OPS, &reg_id, &entity_id, &mock.url(""), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Create entity with local=true → should NOT forward
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Verify no forwarding happened
        pretty_assertions::assert_eq!(mock.get_total_count().await, 0);

        mock.stop();
    }));
}
