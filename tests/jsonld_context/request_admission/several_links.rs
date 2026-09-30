//! A request naming more than one context link is refused (study C1 to C3).

use crate::{
    broker_endpoints::broker_url,
    context_links::{context_link, context_link_targets},
    core_contexts::{CORE_V18, core},
    ngsi_requests::stored_entity,
    private_tenant::PrivateTenant,
    request_admission::{assert_bad_request_data, create_plain_vehicle, requests_for_every_verb},
};
use reqwest::StatusCode;
use serde_json::json;

/// Study C1: two `Link` lines each naming a context are refused on every verb,
/// and nothing is written, changed or deleted.
#[tokio::test]
async fn two_context_link_lines_are_refused_on_every_verb() {
    let tenant = PrivateTenant::provisioned().await;
    let id = create_plain_vehicle(tenant.client()).await;

    for (verb, request) in requests_for_every_verb(tenant.client(), &id) {
        let request = request.header("Link", context_link(core())).header("Link", context_link(CORE_V18));
        assert_bad_request_data(request, &format!("{verb} with two context Link lines")).await;
    }

    let stored = stored_entity(tenant.client(), &id).await;
    assert_eq!(
        stored["speed"]["value"],
        json!(1),
        "the refused PATCH changed nothing and the refused DELETE deleted nothing: {stored}"
    );
    tenant.finish().await;
}

/// Study C2: one `Link` line carrying two context values is two links (IETF
/// RFC 8288 section 3).
#[tokio::test]
async fn one_link_line_carrying_two_context_values_is_refused() {
    let tenant = PrivateTenant::provisioned().await;
    let id = create_plain_vehicle(tenant.client()).await;

    for (verb, request) in requests_for_every_verb(tenant.client(), &id) {
        let both = format!("{}, {}", context_link(core()), context_link(CORE_V18));
        assert_bad_request_data(request.header("Link", both), &format!("{verb} with two context values in one line")).await;
    }
    tenant.finish().await;
}

/// Study C3: JSON-LD counts context links, not distinct targets, so the same
/// link twice is refused too, in two lines and in one.
#[tokio::test]
async fn the_same_context_link_twice_is_refused() {
    let tenant = PrivateTenant::provisioned().await;
    let id = create_plain_vehicle(tenant.client()).await;
    let path = broker_url(&format!("entities/{id}"));

    let two_lines = tenant
        .client()
        .get(&path)
        .header("Link", context_link(core()))
        .header("Link", context_link(core()));
    assert_bad_request_data(two_lines, "the same context link in two lines").await;
    let one_line = tenant
        .client()
        .get(&path)
        .header("Link", format!("{}, {}", context_link(core()), context_link(core())));
    assert_bad_request_data(one_line, "the same context link twice in one line").await;
    tenant.finish().await;
}

/// A POST query sent as `application/json` naming two context links is
/// refused as well, whatever its body says.
#[tokio::test]
async fn a_post_query_naming_two_context_links_is_refused() {
    let tenant = PrivateTenant::provisioned().await;

    let request = tenant
        .client()
        .post(broker_url("entityOperations/query"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(core()))
        .header("Link", context_link(CORE_V18))
        .json(&json!({ "type": "Query", "entities": [{ "type": "Vehicle" }] }));
    assert_bad_request_data(request, "a json POST query with two context links").await;
    tenant.finish().await;
}

/// A request naming one context gets exactly one context link back.
#[tokio::test]
async fn a_request_naming_one_context_is_answered_with_one_context_link() {
    let tenant = PrivateTenant::provisioned().await;
    let id = create_plain_vehicle(tenant.client()).await;

    let response = tenant
        .client()
        .get(broker_url(&format!("entities/{id}")))
        .header("Accept", "application/json")
        .header("Link", context_link(core()))
        .send()
        .await
        .expect("retrieve failed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(context_link_targets(response.headers()), vec![core().to_owned()]);
    tenant.finish().await;
}
