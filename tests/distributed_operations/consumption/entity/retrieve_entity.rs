#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::{
    distributed_operations::prepare_registration,
    harness::{
        client::{QueryParams, RequestHeaders},
        fixtures,
        mock_server::MockServer,
        trial::trial,
    },
};
use libtest_mimic::Trial;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "distributed_operations::consumption::entity::retrieve_entity";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.json";
const VEHICLE_FIXTURE_LD: &str = "entities/vehicle-simple-attributes.jsonld";
const VEHICLE_SECOND_FIXTURE: &str = "entities/vehicle-simple-attributes-second.json";
const VEHICLE_SPEED_FIXTURE: &str = "entities/vehicle-speed-attribute.json";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";
const REG_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-redirection-ops.jsonld";

fn ld_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // D010_01_aux — Retrieve entity on both broker and context source (auxiliary merge)
    trials.push(trial(&format!("{PREFIX}::D010_01_aux::retrieve_entity_auxiliary_merge"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_01_aux");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_01_aux");

        // Create local entity (has brandName=Mercedes, isParked)
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entities?local=true",
                Some(&entity),
                RequestHeaders::new()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create auxiliary registration
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), Some("auxiliary"), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with brandName=BMW and isParked2
        let mock_entity = fixtures::load_fixture(VEHICLE_SECOND_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve entity
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let obj = body.as_object().unwrap();
        let keys: Vec<_> = obj.keys().collect();
        pretty_assertions::assert_eq!(obj.contains_key("brandName"), true, "expected brandName in {keys:?}");
        pretty_assertions::assert_eq!(obj.contains_key("isParked"), true, "expected isParked in {keys:?}");
        pretty_assertions::assert_eq!(obj.contains_key("isParked2"), true, "expected isParked2 in {keys:?}");

        mock.stop();
    }));

    // D010_01_exc — Retrieve entity with exclusive registration
    trials.push(trial(&format!("{PREFIX}::D010_01_exc::retrieve_entity_exclusive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_01_exc");

        // Create local entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive registration with endpoint /broker1
        // The CSR fixture has operations: ["redirectionOps"], which only matches consumption
        // ops in redirect mode. For exclusive mode, we must specify the actual operation.
        let reg = prepare_registration(
            REG_SPEED_REDIRECTION_OPS,
            &reg_id,
            &entity_id,
            &mock.url("/broker1"),
            Some("exclusive"),
            Some(&["retrieveEntity"]),
            None,
        );
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with speed attribute
        let mock_entity = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve entity
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let obj = body.as_object().unwrap();
        pretty_assertions::assert_eq!(
            obj.contains_key("speed"),
            true,
            "expected speed from exclusive source, got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        assert!(mock.get_count(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}")).await > 0);

        mock.stop();
    }));

    // D010_01_inc — Retrieve entity on context source (inclusive)
    trials.push(trial(&format!("{PREFIX}::D010_01_inc::retrieve_entity_inclusive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_01_inc");

        // Create inclusive registration (no mode)
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve entity
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        mock.stop();
    }));

    // D010_01_red — Retrieve entity with redirect (two brokers)
    trials.push(trial(
        &format!("{PREFIX}::D010_01_red::retrieve_entity_redirect_two_brokers"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_01_red");
            let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D010_01_red_1");
            let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D010_01_red_2");

            // Create two redirect registrations
            let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id1, &entity_id, &mock.url("/broker1"), Some("redirect"), None, None);
            let resp = ctx.client.create_registration(&reg1).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id1);

            let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id, &mock.url("/broker2"), Some("redirect"), None, None);
            let resp = ctx.client.create_registration(&reg2).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id2);

            // broker1 returns entity with isParked
            let mock_entity1 = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
            mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity1)
                .await;

            // broker2 returns entity with isParked2
            let mock_entity2 = fixtures::load_fixture(VEHICLE_SECOND_FIXTURE, &entity_id);
            mock.set_stub_with_body(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity2)
                .await;

            // Retrieve entity
            let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

            let body: serde_json::Value = resp.json();
            let obj = body.as_object().unwrap();
            let keys: Vec<_> = obj.keys().collect();
            pretty_assertions::assert_eq!(obj.contains_key("isParked"), true, "expected isParked in {keys:?}");
            pretty_assertions::assert_eq!(obj.contains_key("isParked2"), true, "expected isParked2 in {keys:?}");

            mock.stop();
        },
    ));

    // D010_02_exc — Retrieve entity with exclusive + local=true
    trials.push(trial(&format!("{PREFIX}::D010_02_exc::retrieve_entity_exclusive_local"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_02_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_02_exc");

        // Create local entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive registration
        let reg = prepare_registration(
            REG_SPEED_REDIRECTION_OPS,
            &reg_id,
            &entity_id,
            &mock.url("/broker1"),
            Some("exclusive"),
            None,
            None,
        );
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with speed
        let mock_entity = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve with local=true
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new().local(true), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        pretty_assertions::assert_eq!(
            body.as_object().unwrap().contains_key("speed"),
            false,
            "local=true should not include exclusive speed attribute"
        );

        mock.stop();
    }));

    // D010_02_inc — Retrieve with local=true returns 404 (inclusive, entity only remote)
    trials.push(trial(
        &format!("{PREFIX}::D010_02_inc::retrieve_entity_inclusive_local_404"),
        |ctx| async move {
            let mut mock = MockServer::start().await;
            let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_02_inc");
            let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_02_inc");

            // Create inclusive registration (entity only on remote)
            let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
            let resp = ctx.client.create_registration(&reg).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
            ctx.track_registration(&reg_id);

            // Mock stub (should not be called)
            mock.set_stub_with_body(
                Method::GET,
                &format!("/ngsi-ld/v1/entities/{entity_id}"),
                200,
                &fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id),
            )
            .await;

            // Retrieve with local=true → 404
            let resp = ctx.client.get_entity(&entity_id, &QueryParams::new().local(true), ld_headers()).await;
            pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);

            // Verify no request was forwarded
            pretty_assertions::assert_eq!(mock.get_total_count().await, 0);

            mock.stop();
        },
    ));

    // D010_02_red — Retrieve with local=true returns 404 (redirect)
    trials.push(trial(&format!("{PREFIX}::D010_02_red::retrieve_entity_redirect_local_404"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_02_red");
        let reg_id1 = ctx.entity_id("urn:ngsi-ld:Registration:D010_02_red_1");
        let reg_id2 = ctx.entity_id("urn:ngsi-ld:Registration:D010_02_red_2");

        // Create two redirect registrations
        let reg1 = prepare_registration(REG_REDIRECTION_OPS, &reg_id1, &entity_id, &mock.url("/broker1"), Some("redirect"), None, None);
        let resp = ctx.client.create_registration(&reg1).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id1);

        let reg2 = prepare_registration(REG_REDIRECTION_OPS, &reg_id2, &entity_id, &mock.url("/broker2"), Some("redirect"), None, None);
        let resp = ctx.client.create_registration(&reg2).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id2);

        // Set mock stubs
        mock.set_stub(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}"), 200).await;
        mock.set_stub(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}"), 200).await;

        // Retrieve with local=true → 404
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new().local(true), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::NOT_FOUND);

        // Verify no requests forwarded
        pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/broker1/ngsi-ld/v1/entities/{entity_id}")).await, 0);
        pretty_assertions::assert_eq!(mock.get_count(Method::GET, &format!("/broker2/ngsi-ld/v1/entities/{entity_id}")).await, 0);

        mock.stop();
    }));

    // D010_03_01_inc — Inclusive registration with retrieveEntity operation
    trials.push(trial(&format!("{PREFIX}::D010_03_01_inc::inclusive_retrieve_entity_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_03_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_03_01_inc");

        // Create inclusive registration with retrieveEntity operation
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, Some(&["retrieveEntity"]), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve entity
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        mock.stop();
    }));

    // D010_03_02_inc — Inclusive registration with queryEntity operation
    trials.push(trial(&format!("{PREFIX}::D010_03_02_inc::inclusive_query_entity_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_03_02_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_03_02_inc");

        // Create inclusive registration with queryEntity operation
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, Some(&["queryEntity"]), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // The broker always uses the retrieve path (GET /entities/{id}) regardless of the
        // CSR's operations field — operations only controls CSR matching, not the forwarded path.
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve entity by ID (CSR matches via queryEntity op)
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        mock.stop();
    }));

    // D010_03_03_inc — Inclusive registration with queryBatch operation
    trials.push(trial(&format!("{PREFIX}::D010_03_03_inc::inclusive_query_batch_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D010_03_03_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D010_03_03_inc");

        // Create inclusive registration with queryBatch operation
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, Some(&["queryBatch"]), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // The broker always uses the retrieve path regardless of CSR operations field
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        mock.set_stub_with_body(Method::GET, &format!("/ngsi-ld/v1/entities/{entity_id}"), 200, &mock_entity)
            .await;

        // Retrieve entity by ID (CSR matches via queryBatch op)
        let resp = ctx.client.get_entity(&entity_id, &QueryParams::new(), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        mock.stop();
    }));
}
