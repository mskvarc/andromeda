//! A subscription's `jsonldContext` initialised from a context no single URL
//! names (study D7, D8, D10 and D12).
//!
//! Clause 5.8.1.4 initialises `jsonldContext` "with the @context applicable for
//! the Subscription", and Table 5.2.12-1 types it as one dereferenceable URI.
//! A context that is one URL is stored as written; anything else is hosted as
//! an `ImplicitlyCreated` wrapper of the value an `application/ld+json` body
//! would carry, and the wrapper's absolute URL is stored.

use crate::{
    broker_endpoints::broker_url,
    context_api::{JSON_LD_CONTEXT_PREFIX, dereference, extract_id, implicit_context_ids},
    context_links::context_link_targets,
    core_contexts::core,
    ngsi_requests::{get, jsonld_context_of, ok_body},
    notification_context::{create_notified_device, next_notification, receiver, subscribe},
    private_tenant::PrivateTenant,
    user_contexts::{UserContexts, term_definitions},
};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// Assert `jsonld_context` is the absolute URL of a wrapper hosted for
/// `tenant` holding exactly `expected`, dereferenceable under the tenant's
/// header (clauses 4.14 and 6.3.14).
async fn assert_hosted(jsonld_context: &str, tenant: &PrivateTenant, expected: &Value) {
    assert!(jsonld_context.starts_with("http"), "absolute: {jsonld_context}");
    assert!(
        jsonld_context.contains(&format!("/ngsi-ld/v1/jsonldContexts/{JSON_LD_CONTEXT_PREFIX}")),
        "{jsonld_context}"
    );
    assert!(!jsonld_context.contains('?'), "the URL names no tenant: {jsonld_context}");
    let served = dereference(tenant.client(), jsonld_context).await;
    assert_eq!(served.status, StatusCode::OK, "a receiver dereferences it under the tenant's header");
    assert_eq!(&served.json(), expected);
    assert!(
        implicit_context_ids(tenant.client()).await.contains(&extract_id(jsonld_context)),
        "listed ImplicitlyCreated"
    );
}

/// Study D7 and D12: a subscription created with two contexts in its body is
/// given a hosted wrapper as `jsonldContext`; an ld+json notification names that
/// URL alone (it ends with the core `@context`) and a json one links it.
#[tokio::test]
async fn a_subscription_with_a_context_array_is_notified_under_its_hosted_wrapper() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let array = json!([contexts.terms, contexts.importing]);
    let ld_receiver = receiver().await;
    let json_receiver = receiver().await;

    let ld_subscription = subscribe(tenant.client(), &ld_receiver, "application/ld+json", json!({ "@context": array })).await;
    let json_subscription = subscribe(tenant.client(), &json_receiver, "application/json", json!({ "@context": array })).await;

    let jsonld_context = jsonld_context_of(tenant.client(), &ld_subscription).await;
    assert_eq!(
        jsonld_context,
        jsonld_context_of(tenant.client(), &json_subscription).await,
        "one wrapper for one array"
    );
    assert_hosted(&jsonld_context, &tenant, &json!({ "@context": [contexts.terms, contexts.importing, core()] })).await;
    create_notified_device(tenant.client()).await;
    let notification: Value = next_notification(&ld_receiver).await.json().expect("a JSON notification");
    assert_eq!(notification["@context"], json!(jsonld_context), "{notification}");
    assert_eq!(notification["data"][0]["@context"], json!(jsonld_context), "{notification}");
    let delivered = next_notification(&json_receiver).await;
    assert_eq!(context_link_targets(&delivered.headers), [jsonld_context.as_str()]);
    ld_receiver.shutdown().await.expect("receiver shutdown");
    json_receiver.shutdown().await.expect("receiver shutdown");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study D8: an inline body context is hosted the same way.
#[tokio::test]
async fn a_subscription_with_an_inline_context_is_given_a_hosted_wrapper() {
    let tenant = PrivateTenant::provisioned().await;
    let ld_receiver = receiver().await;

    let subscription = subscribe(tenant.client(), &ld_receiver, "application/ld+json", json!({ "@context": term_definitions() })).await;

    let jsonld_context = jsonld_context_of(tenant.client(), &subscription).await;
    assert_hosted(&jsonld_context, &tenant, &json!({ "@context": [term_definitions(), core()] })).await;
    ld_receiver.shutdown().await.expect("receiver shutdown");
    tenant.finish().await;
}

/// A subscription created under one URL that ends with the core `@context`
/// stores that URL as written, and one created under a user context without
/// the core is given a wrapper of that context followed by the core.
#[tokio::test]
async fn a_subscription_with_one_context_url_stores_it_or_a_wrapper_ending_with_the_core() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let ld_receiver = receiver().await;

    let compound = subscribe(tenant.client(), &ld_receiver, "application/ld+json", json!({ "@context": contexts.wrapper })).await;
    let bare = subscribe(tenant.client(), &ld_receiver, "application/ld+json", json!({ "@context": contexts.terms })).await;

    assert_eq!(jsonld_context_of(tenant.client(), &compound).await, contexts.wrapper);
    let hosted = jsonld_context_of(tenant.client(), &bare).await;
    assert_hosted(&hosted, &tenant, &json!({ "@context": [contexts.terms, core()] })).await;
    ld_receiver.shutdown().await.expect("receiver shutdown");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study D10 and clause 5.11.2.4: a Context Source Registration Subscription
/// created with a context array is given a hosted wrapper too, not the core.
#[tokio::test]
async fn a_csource_subscription_with_a_context_array_is_given_a_hosted_wrapper() {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;

    let created = tenant
        .client()
        .post(broker_url("csourceSubscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "type": "Subscription",
            "entities": [{ "type": "Device" }],
            "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify", "accept": "application/json" } },
            "@context": [contexts.terms, contexts.importing]
        }))
        .send()
        .await
        .expect("create csource subscription failed");
    assert_eq!(created.status(), StatusCode::CREATED);
    let id = extract_id(created.headers().get("location").and_then(|value| value.to_str().ok()).expect("a Location"));

    let stored = ok_body(get(tenant.client(), &format!("csourceSubscriptions/{id}"), "application/json", None).await).await;
    let jsonld_context = stored["jsonldContext"].as_str().expect("a jsonldContext").to_owned();
    assert_hosted(&jsonld_context, &tenant, &json!({ "@context": [contexts.terms, contexts.importing, core()] })).await;
    contexts.shutdown().await;
    tenant.finish().await;
}
