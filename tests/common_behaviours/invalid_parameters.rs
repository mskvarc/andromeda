use crate::harness::{fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "common::invalid_parameters";
const ERROR_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";

pub fn tests(trials: &mut Vec<Trial>) {
    // 059_01_01: POST /entities with invalid query parameter -> 400
    trials.push(trial(&format!("{PREFIX}::059_01_01::create_entity_invalid_param"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:059_01");
        let entity = fixtures::load_fixture("entities/building-minimal.jsonld", &id);

        let resp = ctx
            .client
            .request(Method::POST, "/entities?invalidParam=invalidValue", Some(&entity), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));

    // 059_01_02: GET /entities with invalid query parameter -> 400
    trials.push(trial(&format!("{PREFIX}::059_01_02::query_entities_invalid_param"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/entities?type=Building&invalidParam=invalidValue", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));

    // 059_01_03: POST /temporal/entities with invalid query parameter -> 400
    trials.push(trial(&format!("{PREFIX}::059_01_03::create_temporal_invalid_param"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:059_03");
        let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);

        let resp = ctx
            .client
            .request(Method::POST, "/temporal/entities?invalidParam=invalidValue", Some(&entity), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));

    // 059_01_04: GET /temporal/entities with invalid query parameter -> 400
    trials.push(trial(&format!("{PREFIX}::059_01_04::query_temporal_invalid_param"), |ctx| async move {
        let resp = ctx
            .client
            .request(
                Method::GET,
                "/temporal/entities?type=Building&invalidParam=invalidValue",
                None,
                Default::default(),
            )
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));

    // 059_01_05: GET /types with invalid query parameter -> 400
    trials.push(trial(&format!("{PREFIX}::059_01_05::list_types_invalid_param"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/types?invalidParam=invalidValue", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));

    // 059_01_06: POST /subscriptions with invalid query parameter -> 400
    trials.push(trial(&format!("{PREFIX}::059_01_06::create_subscription_invalid_param"), |ctx| async move {
        let sub_id = format!("urn:ngsi-ld:Subscription:059_06:{}", uuid::Uuid::new_v4());
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &sub_id);

        let resp = ctx
            .client
            .request(Method::POST, "/subscriptions?invalidParam=invalidValue", Some(&sub), Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));

    // 059_01_07: GET /subscriptions with invalid query parameter -> 400
    trials.push(trial(&format!("{PREFIX}::059_01_07::query_subscriptions_invalid_param"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/subscriptions?invalidParam=invalidValue", None, Default::default())
            .await;
        assert_eq!(resp.status, StatusCode::BAD_REQUEST);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));
}
