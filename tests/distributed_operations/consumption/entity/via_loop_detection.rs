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

const PREFIX: &str = "distributed_operations::consumption::entity::via_loop_detection";

const VEHICLE_FIXTURE: &str = "entities/vehicle-simple-attributes.json";
const REG_VEHICLE_COMPLETE: &str = "csource_registrations/context-source-registration-vehicle-complete.jsonld";

fn ld_headers() -> RequestHeaders {
    RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url())
}

pub fn tests(trials: &mut Vec<Trial>) {
    // VIA_01 -- CSR alias in Via header causes CSR to be skipped
    trials.push(trial(&format!("{PREFIX}::VIA_01::csr_alias_in_via_skipped"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:VIA_01");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:VIA_01");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, Some("cs-broker-A"));
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query with Via header containing the CSR's alias -- should NOT forward
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle"), ld_headers().via("1.1 cs-broker-A"))
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities").await;
        pretty_assertions::assert_eq!(count, 0, "mock should not have been called when CSR alias is in Via");

        mock.stop();
    }));

    // VIA_02 -- CSR alias not in Via header allows forwarding
    trials.push(trial(&format!("{PREFIX}::VIA_02::csr_alias_not_in_via_forwarded"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:VIA_02");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:VIA_02");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, Some("cs-broker-A"));
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query with Via header containing a different alias -- should forward
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle"), ld_headers().via("1.1 cs-broker-B"))
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities").await;
        pretty_assertions::assert_eq!(count, 1, "mock should have been called when CSR alias is not in Via");

        mock.stop();
    }));

    // VIA_03 -- No Via header means forwarding proceeds normally
    trials.push(trial(&format!("{PREFIX}::VIA_03::no_via_header_forwarded_normally"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:VIA_03");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:VIA_03");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, Some("cs-broker-A"));
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Query without Via header
        let resp = ctx.client.query_entities(&QueryParams::new().type_("Vehicle"), ld_headers()).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities").await;
        pretty_assertions::assert_eq!(count, 1, "mock should have been called without Via header");

        mock.stop();
    }));

    // VIA_04 -- Substring match does not cause false positive
    trials.push(trial(&format!("{PREFIX}::VIA_04::substring_no_false_positive"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:VIA_04");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:VIA_04");

        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, Some("broker1"));
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Via header contains "broker10" which is NOT "broker1"
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle"), ld_headers().via("1.1 broker10"))
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities").await;
        pretty_assertions::assert_eq!(count, 1, "mock should have been called -- substring match must not trigger");

        mock.stop();
    }));

    // VIA_05 -- CSR without alias is always forwarded regardless of Via
    trials.push(trial(&format!("{PREFIX}::VIA_05::csr_without_alias_always_forwarded"), |ctx| async move {
        let mut mock = MockServer::start().await;
        let entity_id = ctx.entity_id("urn:ngsi-ld:Vehicle:VIA_05");
        let reg_id = ctx.entity_id("urn:ngsi-ld:Registration:VIA_05");

        // No contextSourceAlias
        let reg = prepare_registration(REG_VEHICLE_COMPLETE, &reg_id, &entity_id, &mock.url(""), None, None, None);
        let resp = ctx.client.create_registration(&reg).await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::CREATED);
        ctx.track_registration(&reg_id);

        let mock_entity = fixtures::load_fixture(VEHICLE_FIXTURE, &entity_id);
        let mock_array = serde_json::Value::Array(vec![mock_entity]);
        mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/entities", 200, &mock_array).await;

        // Via header present but CSR has no alias -- should still forward
        let resp = ctx
            .client
            .query_entities(&QueryParams::new().type_("Vehicle"), ld_headers().via("1.1 whatever"))
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        let count = mock.get_count(Method::GET, "/ngsi-ld/v1/entities").await;
        pretty_assertions::assert_eq!(count, 1, "mock should have been called -- CSR has no alias");

        mock.stop();
    }));

    // VIA_06 -- Self-loop detection (broker's own identity in Via)
    trials.push(trial(&format!("{PREFIX}::VIA_06::self_loop_detection"), |ctx| async move {
        // No CSR needed -- the broker's own identity check happens before CSR matching
        // The broker identity is urn:ngsi-ld:ContextSourceIdentity:andromeda by default
        let resp = ctx
            .client
            .query_entities(
                &QueryParams::new().type_("Vehicle"),
                ld_headers().via("1.1 urn:ngsi-ld:ContextSourceIdentity:andromeda"),
            )
            .await;
        pretty_assertions::assert_eq!(resp.status, StatusCode::OK);

        // The response should be an empty array (no local entities, no forwarding)
        let body: Vec<serde_json::Value> = resp.json();
        assert!(body.is_empty(), "self-loop should prevent all forwarding");
    }));
}
