use crate::harness::trial::trial;
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "common::unsupported_media_type";
const DUMMY_BODY: &[u8] = b"<entity/>";

pub fn tests(trials: &mut Vec<Trial>) {
    // 048_01_01: PATCH entity attr with Content-Type application/xml -> 415
    trials.push(trial(&format!("{PREFIX}::048_01_01::partial_update_attr_unsupported"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(Method::PATCH, "/entities/urn:ngsi-ld:Vehicle:dummy/attrs/speed", DUMMY_BODY, "application/xml")
            .await;
        assert_eq!(resp.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }));

    // 048_01_02: PATCH subscription with Content-Type application/xml -> 415
    trials.push(trial(&format!("{PREFIX}::048_01_02::update_subscription_unsupported"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(Method::PATCH, "/subscriptions/urn:ngsi-ld:Subscription:dummy", DUMMY_BODY, "application/xml")
            .await;
        assert_eq!(resp.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }));

    // 048_01_03: POST entity with Content-Type application/xml -> 415
    trials.push(trial(&format!("{PREFIX}::048_01_03::create_entity_unsupported"), |ctx| async move {
        let resp = ctx.client.request_raw(Method::POST, "/entities", DUMMY_BODY, "application/xml").await;
        assert_eq!(resp.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }));

    // 048_01_04: POST subscription with Content-Type application/xml -> 415
    trials.push(trial(&format!("{PREFIX}::048_01_04::create_subscription_unsupported"), |ctx| async move {
        let resp = ctx.client.request_raw(Method::POST, "/subscriptions", DUMMY_BODY, "application/xml").await;
        assert_eq!(resp.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }));

    // 048_01_05: POST batch create with Content-Type application/xml -> 415
    trials.push(trial(&format!("{PREFIX}::048_01_05::batch_create_unsupported"), |ctx| async move {
        let resp = ctx
            .client
            .request_raw(Method::POST, "/entityOperations/create", DUMMY_BODY, "application/xml")
            .await;
        assert_eq!(resp.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }));

    // 048_01_06: POST temporal entity with Content-Type application/xml -> 415
    trials.push(trial(&format!("{PREFIX}::048_01_06::create_temporal_entity_unsupported"), |ctx| async move {
        let resp = ctx.client.request_raw(Method::POST, "/temporal/entities", DUMMY_BODY, "application/xml").await;
        assert_eq!(resp.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }));
}
