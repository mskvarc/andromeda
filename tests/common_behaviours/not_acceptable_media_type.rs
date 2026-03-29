use crate::harness::{client::RequestHeaders, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "common::not_acceptable_media_type";

// ── 049_01: Accept: application/xml -> 406 ────────────────────────────

pub fn tests(trials: &mut Vec<Trial>) {
    // 049_01_01: GET entity with Accept application/xml -> 406
    trials.push(trial(&format!("{PREFIX}::049_01_01::get_entity_not_acceptable"), |ctx| async move {
        let resp = ctx
            .client
            .request(
                Method::GET,
                "/entities/urn:ngsi-ld:Building:dummy",
                None,
                RequestHeaders::new().accept("application/xml"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
    }));

    // 049_01_02: GET subscription with Accept application/xml -> 406
    trials.push(trial(&format!("{PREFIX}::049_01_02::get_subscription_not_acceptable"), |ctx| async move {
        let resp = ctx
            .client
            .request(
                Method::GET,
                "/subscriptions/urn:ngsi-ld:Subscription:dummy",
                None,
                RequestHeaders::new().accept("application/xml"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
    }));

    // 049_01_03: GET csourceRegistrations with Accept application/xml -> 406
    trials.push(trial(&format!("{PREFIX}::049_01_03::query_registrations_not_acceptable"), |ctx| async move {
        let resp = ctx
            .client
            .request(
                Method::GET,
                "/csourceRegistrations?type=Building",
                None,
                RequestHeaders::new().accept("application/xml"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
    }));

    // 049_01_04: GET csourceSubscriptions with Accept application/xml -> 406
    trials.push(trial(&format!("{PREFIX}::049_01_04::query_csr_subs_not_acceptable"), |ctx| async move {
        let resp = ctx
            .client
            .request(Method::GET, "/csourceSubscriptions", None, RequestHeaders::new().accept("application/xml"))
            .await;
        assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
    }));

    // 049_01_05: GET temporal/entities with Accept application/xml -> 406
    trials.push(trial(&format!("{PREFIX}::049_01_05::query_temporal_not_acceptable"), |ctx| async move {
        let resp = ctx
            .client
            .request(
                Method::GET,
                "/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:00:00Z",
                None,
                RequestHeaders::new().accept("application/xml"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
    }));

    // ── 049_02: Accept: application/geo+json -> 406 for non-entity endpoints ──

    // 049_02_01: GET subscription with Accept application/geo+json -> 406
    trials.push(trial(
        &format!("{PREFIX}::049_02_01::get_subscription_geojson_not_acceptable"),
        |ctx| async move {
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    "/subscriptions/urn:ngsi-ld:Subscription:dummy",
                    None,
                    RequestHeaders::new().accept_geo_json(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
        },
    ));

    // 049_02_02: GET temporal/entities with Accept application/geo+json -> 406
    trials.push(trial(
        &format!("{PREFIX}::049_02_02::query_temporal_geojson_not_acceptable"),
        |ctx| async move {
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    "/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:00:00Z",
                    None,
                    RequestHeaders::new().accept_geo_json(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
        },
    ));

    // 049_02_03: GET csourceRegistrations with Accept application/geo+json -> 406
    trials.push(trial(
        &format!("{PREFIX}::049_02_03::query_registrations_geojson_not_acceptable"),
        |ctx| async move {
            let resp = ctx
                .client
                .request(
                    Method::GET,
                    "/csourceRegistrations?type=Building",
                    None,
                    RequestHeaders::new().accept_geo_json(),
                )
                .await;
            assert_eq!(resp.status, StatusCode::NOT_ACCEPTABLE);
        },
    ));
}
