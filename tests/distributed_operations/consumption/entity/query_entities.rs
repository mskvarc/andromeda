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

const PREFIX: &str = "distributed_operations::consumption::entity::query_entities";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.json";
const VEHICLE_FIXTURE_LD: &str = "entities/vehicle-simple-attributes.jsonld";
const VEHICLE_SECOND_FIXTURE: &str = "entities/vehicle-simple-attributes-second.json";
const VEHICLE_SECOND_FIXTURE_LD: &str = "entities/vehicle-simple-attributes-second.jsonld";
const VEHICLE_DIFFERENT_FIXTURE_LD: &str = "entities/vehicle-simple-different-attributes.jsonld";
const VEHICLE_SPEED_FIXTURE: &str = "entities/vehicle-speed-attribute.json";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";
const REG_VEHICLE_ENTITIES: &str = "csource_registrations/context-source-registration-vehicle-entities.jsonld";
const REG_SPEED_REDIRECTION_OPS: &str = "csource_registrations/context-source-registration-vehicle-speed-with-redirection-ops.jsonld";

fn ld_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // D011_01_aux — Query entities with auxiliary mode (merge)
    trials.push(trial(&format!("{PREFIX}::D011_01_aux::query_entities_auxiliary_merge"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_aux");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_aux");

        // Create local entity (has brandName=BMW, isParked2)
        let entity = fixtures::load_fixture(VEHICLE_SECOND_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create auxiliary registration
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), Some("auxiliary"), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with brandName=Mercedes, isParked
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        pretty_assertions::assert_eq!(body.is_empty(), false, "should return at least one entity");
        let first = &body[0];
        let obj = first.as_object().unwrap();
        let keys: Vec<_> = obj.keys().collect();
        pretty_assertions::assert_eq!(obj.contains_key("brandName"), true, "expected brandName in {keys:?}");
        pretty_assertions::assert_eq!(obj.contains_key("isParked"), true, "expected isParked in {keys:?}");
        pretty_assertions::assert_eq!(obj.contains_key("isParked2"), true, "expected isParked2 in {keys:?}");

        mock.stop();
    }));

    // D011_01_exc — Query entities with exclusive mode
    trials.push(trial(&format!("{PREFIX}::D011_01_exc::query_entities_exclusive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_exc");

        // Create local entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive registration — must specify queryEntity explicitly because
        // the fixture's redirectionOps only matches consumption ops in redirect mode
        let reg = prepare_registration(
            REG_SPEED_REDIRECTION_OPS,
            &reg_id,
            &entity_id,
            &mock.url("/broker1"),
            Some("exclusive"),
            Some(&["queryEntity"]),
            None,
        );
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with speed
        let mock_entity = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        let entity = body.iter().find(|e| e["id"] == entity_id).unwrap();
        let obj = entity.as_object().unwrap();
        pretty_assertions::assert_eq!(
            obj.contains_key("speed"),
            true,
            "expected speed from exclusive source, got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        assert!(mock.get_count(Method::GET, "/broker1/ngsi-ld/v1/entities").await > 0);

        mock.stop();
    }));

    // D011_01_red — Query entities with redirect mode
    trials.push(trial(&format!("{PREFIX}::D011_01_red::query_entities_redirect"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_red");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_red");

        // Create redirect registration
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url("/broker1"), Some("redirect"), None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_01_01_inc — Query by type (inclusive)
    trials.push(trial(&format!("{PREFIX}::D011_01_01_inc::query_by_type_inclusive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_01_inc");

        // Create inclusive registration
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_01_02_inc — Query by type with missing attribute (inclusive)
    trials.push(trial(&format!("{PREFIX}::D011_01_02_inc::query_by_type_missing_attr"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_02_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_02_inc");

        // Create inclusive registration
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity without speed attribute
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query for speed attr → empty
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle").attrs(["speed"]), ld_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        pretty_assertions::assert_eq!(body.len(), 0, "no entity should have speed attribute, got {body:?}");

        mock.stop();
    }));

    // D011_01_03_inc — Query by type and attribute (inclusive, two sources)
    trials.push(trial(&format!("{PREFIX}::D011_01_03_inc::query_by_type_and_attr"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id_local = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_03_inc_local");
        let entity_id_remote = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_03_inc_remote");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_03_inc");

        // Create local entity (has brandName, isParked — no isParked2)
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id_local);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create inclusive registration
        let reg = prepare_registration(REG_VEHICLE_ENTITIES, &reg_id, &entity_id_remote, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with isParked2
        let mock_entity = fixtures::load_fixture(VEHICLE_SECOND_FIXTURE, &entity_id_remote);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query for isParked2 attr
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle").attrs(["isParked2"]), ld_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id_remote), "should contain remote entity with isParked2");

        mock.stop();
    }));

    // D011_01_04_inc — Query by type with attribute in neither source
    trials.push(trial(&format!("{PREFIX}::D011_01_04_inc::query_by_type_attr_in_neither"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id_local = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_04_inc_local");
        let entity_id_remote = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_04_inc_remote");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_04_inc");

        // Create local entity (has brandName, isParked2 — no speed)
        let entity = fixtures::load_fixture(VEHICLE_SECOND_FIXTURE_LD, &entity_id_local);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create inclusive registration
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id_remote, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity without speed
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id_remote);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query for speed attr → empty
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle").attrs(["speed"]), ld_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        pretty_assertions::assert_eq!(body.len(), 0, "neither entity has speed attribute, got {body:?}");

        mock.stop();
    }));

    // D011_01_05_inc — Query merges same-ID entity from both sources
    trials.push(trial(&format!("{PREFIX}::D011_01_05_inc::query_merge_same_id"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_01_05_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_01_05_inc");

        // Create local entity (has speed, isParked2)
        let entity = fixtures::load_fixture(VEHICLE_DIFFERENT_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create inclusive registration
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with brandName, isParked (same ID)
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        // Should have single merged entity
        let entity = body.iter().find(|e| e["id"] == entity_id).unwrap();
        let obj = entity.as_object().unwrap();
        let keys: Vec<_> = obj.keys().collect();
        pretty_assertions::assert_eq!(obj.contains_key("brandName"), true, "expected brandName in {keys:?}");
        pretty_assertions::assert_eq!(obj.contains_key("speed"), true, "expected speed in {keys:?}");
        pretty_assertions::assert_eq!(obj.contains_key("isParked"), true, "expected isParked in {keys:?}");
        pretty_assertions::assert_eq!(obj.contains_key("isParked2"), true, "expected isParked2 in {keys:?}");

        mock.stop();
    }));

    // D011_02_01_exc — Exclusive with queryEntity operation
    trials.push(trial(&format!("{PREFIX}::D011_02_01_exc::exclusive_query_entity_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_02_01_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_02_01_exc");

        // Create local entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive registration with queryEntity operation
        let reg = prepare_registration(
            REG_SPEED_REDIRECTION_OPS,
            &reg_id,
            &entity_id,
            &mock.url("/broker1"),
            Some("exclusive"),
            Some(&["queryEntity"]),
            None,
        );
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity with speed
        let mock_entity = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_02_02_exc — Exclusive with queryBatch operation
    trials.push(trial(&format!("{PREFIX}::D011_02_02_exc::exclusive_query_batch_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_02_02_exc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_02_02_exc");

        // Create local entity
        let entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let resp = ctx
            .client
            .request(Method::POST, "/entities?local=true", Some(&entity), RequestHeaders::new())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);

        // Create exclusive registration with queryBatch operation
        let reg = prepare_registration(
            REG_SPEED_REDIRECTION_OPS,
            &reg_id,
            &entity_id,
            &mock.url("/broker1"),
            Some("exclusive"),
            Some(&["queryBatch"]),
            None,
        );
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // The broker always forwards queries as GET /entities regardless of CSR operations
        let mock_entity = fixtures::load_fixture(VEHICLE_SPEED_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_02_inc — Query with local=true (inclusive, no local entities)
    trials.push(trial(&format!("{PREFIX}::D011_02_inc::query_inclusive_local_empty"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_02_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_02_inc");

        // Create inclusive registration (no local entity)
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock stub
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE_LD, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query with local=true → empty
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle").local(true), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        pretty_assertions::assert_eq!(body.len(), 0, "local=true should return empty when no local entities");

        mock.stop();
    }));

    // D011_02_01_red — Redirect with queryEntity operation
    trials.push(trial(&format!("{PREFIX}::D011_02_01_red::redirect_query_entity_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_02_01_red");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_02_01_red");

        // Create redirect registration with queryEntity operation
        let reg = prepare_registration(
            REG_VEHICLE_COMPLETE,
            &reg_id,
            &entity_id,
            &mock.url("/broker1"),
            Some("redirect"),
            Some(&["queryEntity"]),
            None,
        );
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_02_02_red — Redirect with queryBatch operation
    trials.push(trial(&format!("{PREFIX}::D011_02_02_red::redirect_query_batch_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_02_02_red");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_02_02_red");

        // Create redirect registration with queryBatch operation
        let reg = prepare_registration(
            REG_VEHICLE_COMPLETE,
            &reg_id,
            &entity_id,
            &mock.url("/broker1"),
            Some("redirect"),
            Some(&["queryBatch"]),
            None,
        );
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // The broker always forwards queries as GET /entities regardless of CSR operations
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/broker1/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query entities
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_03_01_inc — Query by entity ID with queryEntity operation
    trials.push(trial(&format!("{PREFIX}::D011_03_01_inc::query_by_id_query_entity_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_03_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_03_01_inc");

        // Create inclusive registration with queryEntity operation
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, Some(&["queryEntity"]), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query by ID
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle").id(&entity_id), ld_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_03_02_inc — Query by entity ID with queryBatch operation
    trials.push(trial(&format!("{PREFIX}::D011_03_02_inc::query_by_id_query_batch_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_03_02_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_03_02_inc");

        // Create inclusive registration with queryBatch operation
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, Some(&["queryBatch"]), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // The broker always forwards queries as GET /entities regardless of CSR operations
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query by ID
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle").id(&entity_id), ld_headers())
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_04_01_inc — Query via POST with queryBatch operation
    trials.push(trial(&format!("{PREFIX}::D011_04_01_inc::query_via_post_query_batch_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_04_01_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_04_01_inc");

        // Create inclusive registration with queryBatch operation
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, Some(&["queryBatch"]), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // The broker always forwards queries as GET /entities regardless of CSR operations
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query via POST
        let query_body = serde_json::json!({
            "entities": [{"id": entity_id, "type": "Vehicle"}]
        });
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entityOperations/query",
                Some(&query_body),
                RequestHeaders::new()
                    .accept_ld_json()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));

    // D011_04_02_inc — Query via POST with queryEntity operation
    trials.push(trial(&format!("{PREFIX}::D011_04_02_inc::query_via_post_query_entity_op"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:D011_04_02_inc");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:D011_04_02_inc");

        // Create inclusive registration with queryEntity operation
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, Some(&["queryEntity"]), None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        // Mock returns entity
        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query via POST
        let query_body = serde_json::json!({
            "entities": [{"id": entity_id, "type": "Vehicle"}]
        });
        let resp = ctx
            .client
            .request(
                Method::POST,
                "/entityOperations/query",
                Some(&query_body),
                RequestHeaders::new()
                    .accept_ld_json()
                    .content_type("application/json")
                    .link_context(fixtures::test_suite_context_url()),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.iter().any(|e| e["id"] == entity_id));

        mock.stop();
    }));
}
