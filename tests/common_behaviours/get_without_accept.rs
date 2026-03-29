use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "common::get_without_accept";

fn assert_json_content_type(resp: &crate::harness::client::NgsiLdResponse) {
    let ct = resp.headers.get("content-type").expect("should have Content-Type header").to_str().unwrap();
    assert!(ct.contains("application/json"), "Content-Type should be application/json, got {ct}");
}

fn assert_has_link_header(resp: &crate::harness::client::NgsiLdResponse) {
    assert!(resp.headers.get("link").is_some(), "should have Link header");
}

pub fn tests(trials: &mut Vec<Trial>) {
    // 045_01_01: GET entity with Accept */* returns application/json + Link header
    trials.push(trial(&format!("{PREFIX}::045_01_01::get_entity_without_accept"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:045_01_01");
        let entity = fixtures::load_fixture("entities/building-simple-attributes.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(Method::GET, &format!("/entities/{id}"), None, RequestHeaders::new().accept("*/*"))
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_json_content_type(&resp);
        assert_has_link_header(&resp);
    }));

    // 045_01_02: GET subscription with Accept */* returns application/json + Link header
    trials.push(trial(&format!("{PREFIX}::045_01_02::get_subscription_without_accept"), |ctx| async move {
        let sub_id = format!("urn:ngsi-ld:Subscription:045_01_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &sub_id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(Method::GET, &format!("/subscriptions/{sub_id}"), None, RequestHeaders::new().accept("*/*"))
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_json_content_type(&resp);
        assert_has_link_header(&resp);
    }));

    // 045_01_03: GET csourceRegistrations with Accept */* returns application/json + Link header
    trials.push(trial(&format!("{PREFIX}::045_01_03::query_registrations_without_accept"), |ctx| async move {
        let reg_id = format!("urn:ngsi-ld:ContextSourceRegistration:045_01_03:{}", uuid::Uuid::new_v4());
        ctx.track_registration(&reg_id);
        let reg = fixtures::load_fixture("csource_registrations/context-source-registration-with-expiration.jsonld", &reg_id);

        let resp = ctx.client.create_registration(&reg).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let resp = ctx
            .client
            .request(Method::GET, "/csourceRegistrations?type=Vehicle", None, RequestHeaders::new().accept("*/*"))
            .await;
        assert_eq!(resp.status, StatusCode::OK);
        assert_json_content_type(&resp);
        assert_has_link_header(&resp);
    }));

    // 045_01_04: GET temporal/entities with Accept */* returns application/json + Link header
    trials.push(trial(
        &format!("{PREFIX}::045_01_04::query_temporal_entities_without_accept"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Vehicle:045_01_04");
            ctx.track_temporal_entity(&id);
            let entity = fixtures::load_fixture("temporal_entities/vehicle-temporal-representation.jsonld", &id);

            let resp = ctx.client.create_temporal_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::CREATED);

            let resp = ctx
                .client
                .request(
                    Method::GET,
                    "/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:00:00Z",
                    None,
                    RequestHeaders::new().accept("*/*"),
                )
                .await;
            assert_eq!(resp.status, StatusCode::OK);
            assert_json_content_type(&resp);
            assert_has_link_header(&resp);
        },
    ));
}
