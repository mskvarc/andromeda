//! The forms a single context `Link` may take (study C4, C5 and C7 to C9).

use crate::{
    broker_endpoints::broker_url,
    context_api::add_hosted,
    context_links::{CONTEXT_RELATION, context_link, context_link_targets, link_with_relation, profile_link, single_context_link},
    context_origin::ContextOrigin,
    core_contexts::{CORE_V13, CORE_V16, CORE_V18, CORE_VERSIONLESS, core, core_value},
    ngsi_requests::stored_entity,
    private_tenant::PrivateTenant,
    request_admission::{assert_bad_request_data, create_plain_vehicle, speed_context},
    unique_ids::unique_id,
};
use reqwest::{StatusCode, header::HeaderValue};
use serde_json::{Value, json};

/// Study C4: a `profile` link beside the one context link is not a context,
/// and the request is answered with one context link.
#[tokio::test]
async fn a_context_link_beside_a_profile_link_is_admitted() {
    let tenant = PrivateTenant::provisioned().await;
    let id = create_plain_vehicle(tenant.client()).await;

    let response = tenant
        .client()
        .get(broker_url(&format!("entities/{id}")))
        .header("Accept", "application/json")
        .header("Link", context_link(core()))
        .header("Link", profile_link())
        .send()
        .await
        .expect("retrieve failed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(context_link_targets(response.headers()), vec![core().to_owned()]);
    tenant.finish().await;
}

/// Study C5: the relation is compared case-insensitively, so a capitalised
/// context relation names the context the body is expanded through, and it
/// counts toward the one-link limit.
#[tokio::test]
async fn a_capitalised_context_relation_names_the_context_and_counts() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let context = origin.serve_document("/capitalised.jsonld", &speed_context("https://a.example/speed")).await;
    let capitalised = link_with_relation(&context, "HTTP://WWW.W3.ORG/ns/json-ld#context");
    let id = unique_id("urn:ngsi-ld:Vehicle:capitalised");

    let created = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .header("Link", capitalised.as_str())
        .json(&json!({ "id": id, "type": "Vehicle", "speed": { "type": "Property", "value": 7 } }))
        .send()
        .await
        .expect("create failed");
    assert_eq!(created.status(), StatusCode::CREATED);

    let stored = stored_entity(tenant.client(), &id).await;
    assert_eq!(
        stored["https://a.example/speed"]["value"],
        json!(7),
        "the capitalised relation's context expanded the term: {stored}"
    );
    let counted = tenant
        .client()
        .get(broker_url(&format!("entities/{id}")))
        .header("Link", capitalised)
        .header("Link", context_link(core()));
    assert_bad_request_data(counted, "a capitalised context link counts toward the limit").await;
    origin.shutdown().await;
    tenant.finish().await;
}

/// Study C7: a malformed `Link` and a `Link` that is not visible ASCII are
/// refused, on a body-less request and on a body-carrying one.
#[tokio::test]
async fn a_malformed_or_non_ascii_link_is_refused_with_or_without_a_body() {
    let tenant = PrivateTenant::provisioned().await;
    let id = create_plain_vehicle(tenant.client()).await;
    let non_ascii =
        HeaderValue::from_bytes(format!("<https://example.org/\u{e9}.jsonld>; rel=\"{CONTEXT_RELATION}\"").as_bytes()).expect("an opaque header value");

    for (label, value) in [("malformed", HeaderValue::from_static("this is not a link")), ("non-ASCII", non_ascii)] {
        let body_less = tenant.client().get(broker_url(&format!("entities/{id}"))).header("Link", value.clone()); // clone: the same value is sent twice
        assert_bad_request_data(body_less, &format!("a {label} Link on GET")).await;
        let with_body = tenant
            .client()
            .post(broker_url("entityOperations/query"))
            .header("Content-Type", "application/json")
            .header("Link", value)
            .json(&json!({ "type": "Query", "entities": [{ "type": "Vehicle" }] }));
        assert_bad_request_data(with_body, &format!("a {label} Link on POST query")).await;
    }
    tenant.finish().await;
}

/// Study C8: a single `Link` naming any core release, or the versionless
/// alias, is admitted and answered under the v1.9 core context (clause 4.4),
/// which is the only one a response names.
#[tokio::test]
async fn a_single_link_to_any_core_release_is_answered_under_the_current_one() {
    let tenant = PrivateTenant::provisioned().await;
    let id = create_plain_vehicle(tenant.client()).await;

    for release in [CORE_V18, CORE_V16, CORE_V13, CORE_VERSIONLESS, core()] {
        let plain = tenant
            .client()
            .get(broker_url(&format!("entities/{id}")))
            .header("Accept", "application/json")
            .header("Link", context_link(release))
            .send()
            .await
            .expect("retrieve failed");
        assert_eq!(plain.status(), StatusCode::OK, "{release}");
        assert_eq!(single_context_link(plain.headers()), core(), "{release}");

        let ld = tenant
            .client()
            .get(broker_url(&format!("entities/{id}")))
            .header("Accept", "application/ld+json")
            .header("Link", context_link(release))
            .send()
            .await
            .expect("retrieve failed");
        let body: Value = ld.json().await.expect("an entity body");
        assert_eq!(body["@context"], core_value(), "{release}");
        assert_eq!(body["speed"]["value"], json!(1), "{release}: {body}");
    }
    tenant.finish().await;
}

/// Study C9: a relative context `Link` is resolved against the request's own
/// URI (IETF RFC 8288 section 3.1): a root-relative reference to a context the
/// tenant stores, the `Location` Add @context answers, expands the request's
/// terms.
#[tokio::test]
async fn a_relative_context_link_is_resolved_against_the_request_uri() {
    let tenant = PrivateTenant::provisioned().await;
    let hosted = add_hosted(tenant.client(), &json!({ "@context": { "speed": "https://relative.example/speed" } })).await;
    let relative = format!("/ngsi-ld/v1/jsonldContexts/{hosted}");
    let id = unique_id("urn:ngsi-ld:Vehicle:relative");

    let created = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(&relative))
        .json(&json!({ "id": id, "type": "Vehicle", "speed": { "type": "Property", "value": 3 } }))
        .send()
        .await
        .expect("create failed");

    assert_eq!(created.status(), StatusCode::CREATED, "{:?}", created.text().await);
    let stored = stored_entity(tenant.client(), &id).await;
    assert_eq!(
        stored["https://relative.example/speed"]["value"],
        json!(3),
        "the hosted context expanded the term: {stored}"
    );
    tenant.finish().await;
}
