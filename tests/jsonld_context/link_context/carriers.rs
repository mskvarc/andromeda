//! Which responses carry the context link, and which never host a wrapper
//!.
//!
//! Clause 6.3.6: "Operations where the response payload body is not present
//! such as successful HTTP POST, PATCH, PUT or DELETE operations and all error
//! responses, do not include the Link header in the response", and a 207 body
//! uses fully qualified names only "as there is no context present". A wrapper
//! is hosted only when a response names its context in a `Link` and has a body.

use crate::{
    broker_endpoints::broker_url,
    context_api::implicit_context_ids,
    context_links::{context_link, context_link_targets, single_context_link},
    link_context::assert_no_context_link,
    ngsi_requests::{create_device, get},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{DEVICE_IRI, UserContexts},
};
use reqwest::StatusCode;
use serde_json::json;

/// A paginated json query names exactly one context link across
/// every `Link` line, beside its pagination links.
#[tokio::test]
async fn a_paginated_json_query_names_one_context_link_beside_its_pagination_links() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    for _ in 0..3 {
        create_device(tenant.client(), &unique_id("urn:ngsi-ld:Device:paged"), &contexts.terms).await;
    }

    for shape in [&contexts.terms, &contexts.wrapper] {
        let response = get(tenant.client(), "entities?type=Device&limit=1&count=true", "application/json", Some(shape)).await;
        assert_eq!(response.status(), StatusCode::OK, "{shape}");
        let links: Vec<String> = response
            .headers()
            .get_all("link")
            .iter()
            .map(|value| value.to_str().expect("a visible-ASCII Link").to_owned())
            .collect();
        assert!(
            links.iter().any(|line| line.contains(r#"rel="next""#) || line.contains("rel=next")),
            "a next link: {links:?}"
        );
        assert_eq!(context_link_targets(response.headers()).len(), 1, "one context link among {links:?}");
    }
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Body-less 201 and 204 answers name no context and host nothing,
/// whatever context the request named.
#[tokio::test]
async fn body_less_answers_name_no_context_and_host_nothing() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let id = unique_id("urn:ngsi-ld:Device:bodyless");

    let created = tenant
        .client()
        .post(broker_url("entities"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(&contexts.terms))
        .json(&json!({ "id": id, "type": "Device", "speed": { "type": "Property", "value": 1 } }))
        .send()
        .await
        .expect("create failed");
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_no_context_link(created.headers(), "a 201 without a body");

    let patched = tenant
        .client()
        .patch(broker_url(&format!("entities/{id}/attrs")))
        .header("Content-Type", "application/json")
        .header("Link", context_link(&contexts.terms))
        .json(&json!({ "speed": { "type": "Property", "value": 2 } }))
        .send()
        .await
        .expect("update failed");
    assert_eq!(patched.status(), StatusCode::NO_CONTENT);
    assert_no_context_link(patched.headers(), "a 204");

    let deleted = tenant
        .client()
        .delete(broker_url(&format!("entities/{id}")))
        .header("Link", context_link(&contexts.terms))
        .send()
        .await
        .expect("delete failed");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert_no_context_link(deleted.headers(), "a 204 delete");

    assert!(implicit_context_ids(tenant.client()).await.is_empty(), "no wrapper was hosted");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// A 207 and an error name no context and host nothing.
#[tokio::test]
async fn partial_successes_and_errors_name_no_context_and_host_nothing() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;

    let partial = tenant
        .client()
        .post(broker_url("entityOperations/upsert"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("Link", context_link(&contexts.terms))
        .json(&json!([
            { "id": unique_id("urn:ngsi-ld:Device:partial"), "type": "Device", "speed": { "type": "Property", "value": 1 } },
            { "id": unique_id("urn:ngsi-ld:Device:refused"), "type": "Device", "speed": { "type": "Property", "value": null } }
        ]))
        .send()
        .await
        .expect("batch upsert failed");
    // Clause 5.5.4 refuses a Property whose value is null, one Entity at a time.
    assert_eq!(partial.status(), StatusCode::MULTI_STATUS);
    assert_no_context_link(partial.headers(), "a 207");

    let missing = get(
        tenant.client(),
        &format!("entities/{}", unique_id("urn:ngsi-ld:Device:missing")),
        "application/json",
        Some(&contexts.terms),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_no_context_link(missing.headers(), "an error");

    assert!(implicit_context_ids(tenant.client()).await.is_empty(), "no wrapper was hosted");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// An `application/ld+json` answer carries its context in the body
/// and never hosts a wrapper, whatever context the request named.
#[tokio::test]
async fn an_ld_json_answer_never_hosts_a_wrapper() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:ld-no-wrapper");
    create_device(tenant.client(), &device, &contexts.terms).await;

    for shape in [&contexts.terms, &contexts.importing, &contexts.outer] {
        let response = get(tenant.client(), &format!("entities/{device}"), "application/ld+json", Some(shape)).await;
        assert_eq!(response.status(), StatusCode::OK, "{shape}");
        assert_no_context_link(response.headers(), "an ld+json answer");
    }
    assert!(implicit_context_ids(tenant.client()).await.is_empty(), "no wrapper was hosted");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// The temporal POST query answered as `application/json` names one context
/// link: the body context itself when it is one URL ending with the core
/// `@context`, a wrapper otherwise.
#[tokio::test]
async fn a_temporal_post_query_answered_as_json_names_one_context_link() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:temporal-json");
    create_device(tenant.client(), &device, &contexts.terms).await;

    for (context, hosted) in [(json!(contexts.wrapper), false), (json!([contexts.terms, contexts.importing]), true)] {
        let response = tenant
            .client()
            .post(broker_url("temporal/entityOperations/query"))
            .header("Content-Type", "application/ld+json")
            .header("Accept", "application/json")
            .json(&json!({
                "type": "Query",
                "entities": [{ "type": DEVICE_IRI, "id": device }],
                "temporalQ": { "timerel": "after", "timeAt": "2000-01-01T00:00:00Z", "timeproperty": "modifiedAt" },
                "@context": context
            }))
            .send()
            .await
            .expect("temporal POST query failed");
        assert_eq!(response.status(), StatusCode::OK, "{context}");
        let linked = single_context_link(response.headers());
        assert_eq!(linked.contains("/ngsi-ld/v1/jsonldContexts/"), hosted, "{context}: {linked}");
    }
    contexts.shutdown().await;
    tenant.finish().await;
}

/// A batch create answered 201 names at most one context link.
#[tokio::test]
async fn a_batch_create_answer_names_at_most_one_context_link() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;

    let created = tenant
        .client()
        .post(broker_url("entityOperations/create"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(&contexts.terms))
        .json(&json!([
            { "id": unique_id("urn:ngsi-ld:Device:batch-one"), "type": "Device" },
            { "id": unique_id("urn:ngsi-ld:Device:batch-two"), "type": "Device" }
        ]))
        .send()
        .await
        .expect("batch create failed");

    assert_eq!(created.status(), StatusCode::CREATED);
    assert!(context_link_targets(created.headers()).len() <= 1, "{:?}", created.headers());
    contexts.shutdown().await;
    tenant.finish().await;
}
