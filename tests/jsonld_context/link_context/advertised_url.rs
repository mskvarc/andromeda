//! Where minted URLs point (study B13).
//!
//! Every URL the broker mints for a stored `@context` is absolute and built on
//! one apiRoot: the configured `[identity] advertised_url`, path prefix
//! included (clause 6.2 allows "an optional prefix string"), and otherwise the
//! scheme and authority the request was addressed to. The `Location` of Add
//! @context stays the relative path clause 6.29.3.1 prescribes. A reference
//! under the configured prefix names the broker's own document as well.

use crate::{
    broker_endpoints::broker_url,
    context_api::{JSON_LD_CONTEXT_PREFIX, dereference, extract_id},
    context_links::{context_link, single_context_link},
    core_contexts::core,
    ngsi_requests::{create_device, get, jsonld_context_of},
    private_tenant::PrivateTenant,
    unique_ids::unique_id,
    user_contexts::{UserContexts, term_definitions},
};
use andromeda_test_harness::broker_process::BrokerProcess;
use reqwest::{StatusCode, header::LOCATION};
use serde_json::{Value, json};
use std::path::Path;

/// The configured apiRoot, with a path prefix.
const ADVERTISED: &str = "https://gw.example.com/broker";

/// An origin no name service resolves (IETF RFC 6761 section 6.4).
const UNREACHABLE_ORIGIN: &str = "https://advertised-reference.invalid";

/// Without a configured apiRoot, a wrapper URL is built on the scheme and
/// authority the request was addressed to.
#[tokio::test]
async fn without_an_advertised_url_a_wrapper_is_minted_on_the_request_origin() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let device = unique_id("urn:ngsi-ld:Device:request-origin");
    create_device(tenant.client(), &device, &contexts.terms).await;

    let wrapper = single_context_link(
        get(tenant.client(), &format!("entities/{device}"), "application/json", Some(&contexts.terms))
            .await
            .headers(),
    );

    let origin = broker_url("");
    let origin = origin.split("/ngsi-ld/").next().expect("the broker origin");
    assert!(
        wrapper.starts_with(&format!("{origin}/ngsi-ld/v1/jsonldContexts/{JSON_LD_CONTEXT_PREFIX}")),
        "{wrapper}"
    );
    contexts.shutdown().await;
    tenant.finish().await;
}

/// With a configured apiRoot, every minted URL is built on it, prefix
/// included: a response wrapper, a subscription's `jsonldContext`, and the
/// serving URL of a Hosted @context; `Location` stays relative; and a
/// reference under the prefix, on any origin, is resolved from storage.
#[tokio::test]
async fn with_an_advertised_url_every_minted_url_is_built_on_it() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let broker = BrokerProcess::start(
        Path::new(env!("CARGO_BIN_EXE_andromeda")),
        &[("ANDROMEDA__IDENTITY__ADVERTISED_URL", ADVERTISED)],
    )
    .await
    .expect("the broker starts");
    let client = tenant.client();
    let device = unique_id("urn:ngsi-ld:Device:advertised");
    create_device(client, &device, &contexts.terms).await;
    let minted_prefix = format!("{ADVERTISED}/ngsi-ld/v1/jsonldContexts/{JSON_LD_CONTEXT_PREFIX}");

    let answered = client
        .get(broker.url(&format!("entities/{device}")))
        .header("Accept", "application/json")
        .header("Link", context_link(&contexts.terms))
        .send()
        .await
        .expect("retrieval failed");
    let wrapper = single_context_link(answered.headers());
    assert!(wrapper.starts_with(&minted_prefix) && !wrapper.contains('?'), "{wrapper}");
    let served = dereference(client, &broker.url(&format!("jsonldContexts/{}", extract_id(&wrapper)))).await;
    assert_eq!(served.json(), json!({ "@context": [contexts.terms, core()] }));

    let subscribed = client
        .post(broker.url("subscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "type": "Subscription",
            "entities": [{ "type": "Device" }],
            "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify" } },
            "@context": [term_definitions(), { "rate": "https://example.org/rate" }]
        }))
        .send()
        .await
        .expect("create subscription failed");
    assert_eq!(subscribed.status(), StatusCode::CREATED);
    let subscription = extract_id(subscribed.headers().get(LOCATION).and_then(|value| value.to_str().ok()).expect("a Location"));
    let jsonld_context = jsonld_context_of(client, &subscription).await;
    assert!(jsonld_context.starts_with(&minted_prefix) && !jsonld_context.contains('?'), "{jsonld_context}");

    let added = client
        .post(broker.url("jsonldContexts"))
        .header("Content-Type", "application/json")
        .json(&json!({ "@context": { "advertisedSpeed": "https://example.org/advertised/speed" } }))
        .send()
        .await
        .expect("add context failed");
    assert_eq!(added.status(), StatusCode::CREATED);
    let location = added
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .expect("a Location")
        .to_owned();
    assert!(
        location.starts_with("/ngsi-ld/v1/jsonldContexts/"),
        "clause 6.29.3.1: a relative Location: {location}"
    );
    let hosted = extract_id(&location);
    let details: Value = client
        .get(broker.url(&format!("jsonldContexts/{}?details=true", urlencoding::encode(&hosted))))
        .send()
        .await
        .expect("details failed")
        .json()
        .await
        .expect("details");
    let serving = details["URL"].as_str().expect("a serving URL");
    assert!(serving.starts_with(&minted_prefix) && !serving.contains('?'), "{serving}");

    for reference in [
        format!("{UNREACHABLE_ORIGIN}/broker/ngsi-ld/v1/jsonldContexts/{hosted}"),
        format!("{UNREACHABLE_ORIGIN}/ngsi-ld/v1/jsonldContexts/{hosted}"),
    ] {
        let id = unique_id("urn:ngsi-ld:Device:advertised-reference");
        let created = client
            .post(broker.url("entities"))
            .header("Content-Type", "application/json")
            .header("Link", context_link(&reference))
            .json(&json!({ "id": id, "type": "Device", "advertisedSpeed": { "type": "Property", "value": 9 } }))
            .send()
            .await
            .expect("create failed");
        assert_eq!(created.status(), StatusCode::CREATED, "{reference}: {:?}", created.text().await);
    }
    drop(broker);
    contexts.shutdown().await;
    tenant.finish().await;
}
