//! A wrapper that cannot be stored fails the request (study B16 and D11).
//!
//! A response that names its context through a wrapper the broker could not
//! store would link a URL nobody can dereference, and a subscription whose
//! `jsonldContext` could not be hosted would be notified under a context the
//! client never asked for (clause 5.8.1.4). Both fail with a problem detail
//! instead: 503 when the store is saturated, 500 otherwise, never a 200 without
//! its link and never a stored subscription.
//!
//! The failure is made deterministic by giving the private tenant's own
//! `jsonld_contexts` table a constraint that refuses every new
//! `ImplicitlyCreated` row (kind 3); nothing else in the tenant is touched.

use crate::{
    broker_endpoints::broker_url,
    context_links::context_link_targets,
    ngsi_requests::{create_device, get, ok_body},
    private_tenant::PrivateTenant,
    problem_types::INTERNAL_ERROR_TYPE,
    unique_ids::unique_id,
    user_contexts::{DEVICE_IRI, UserContexts, term_definitions},
};
use reqwest::StatusCode;
use serde_json::{Value, json};

/// Make the tenant's store refuse every new `ImplicitlyCreated` row.
async fn refuse_implicit_contexts(tenant: &PrivateTenant) {
    let database = tenant.database().await;
    sqlx::query("ALTER TABLE jsonld_contexts ADD CONSTRAINT jsonld_context_suite_refuses_implicit CHECK (kind <> 3) NOT VALID")
        .execute(&database)
        .await
        .expect("the constraint is added");
    database.close().await;
}

/// Assert `status` and `problem` report a failed store, not a client error.
fn assert_store_failure(status: StatusCode, problem: &Value) {
    assert!(
        status == StatusCode::INTERNAL_SERVER_ERROR || status == StatusCode::SERVICE_UNAVAILABLE,
        "a store failure is a 500 or a 503: {status} {problem}"
    );
    if status == StatusCode::INTERNAL_SERVER_ERROR {
        assert_eq!(problem["type"], INTERNAL_ERROR_TYPE.as_str(), "{problem}");
    }
}

/// Study B16: a json answer whose wrapper cannot be stored is a problem
/// detail, never a 200 without its link; the same request as ld+json, which
/// needs no wrapper, is answered.
#[tokio::test]
async fn a_json_answer_whose_wrapper_cannot_be_stored_fails() {
    let tenant = PrivateTenant::provisioned().await;
    let device = unique_id("urn:ngsi-ld:Device:hosting-failure");
    let contexts = UserContexts::start().await;
    create_device(tenant.client(), &device, &contexts.terms).await;
    refuse_implicit_contexts(&tenant).await;
    let query = json!({ "type": "Query", "entities": [{ "type": DEVICE_IRI, "id": device }], "@context": term_definitions() });

    let refused = tenant
        .client()
        .post(broker_url("entityOperations/query"))
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/json")
        .json(&query)
        .send()
        .await
        .expect("POST query failed");
    let status = refused.status();
    assert!(context_link_targets(refused.headers()).is_empty(), "a failure names no context");
    let problem: Value = refused.json().await.unwrap_or_default();
    assert_store_failure(status, &problem);

    let answered = tenant
        .client()
        .post(broker_url("entityOperations/query"))
        .header("Content-Type", "application/ld+json")
        .header("Accept", "application/ld+json")
        .json(&query)
        .send()
        .await
        .expect("POST query failed");
    let body = ok_body(answered).await;
    assert_eq!(body[0]["id"], json!(device), "an ld+json answer needs no wrapper: {body}");
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Study D11: a subscription whose `@context` needs a wrapper that cannot be
/// stored is refused, and no subscription is stored.
#[tokio::test]
async fn a_subscription_whose_context_cannot_be_hosted_is_refused_and_not_stored() {
    let tenant = PrivateTenant::provisioned().await;
    refuse_implicit_contexts(&tenant).await;
    let id = unique_id("urn:ngsi-ld:Subscription:hosting-failure");

    let refused = tenant
        .client()
        .post(broker_url("subscriptions"))
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "id": id,
            "type": "Subscription",
            "entities": [{ "type": "Device" }],
            "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify" } },
            "@context": [term_definitions(), { "rate": "https://example.org/rate" }]
        }))
        .send()
        .await
        .expect("create subscription failed");
    let status = refused.status();
    let problem: Value = refused.json().await.unwrap_or_default();
    assert_store_failure(status, &problem);

    let absent = get(tenant.client(), &format!("subscriptions/{id}"), "application/json", None).await;
    assert_eq!(absent.status(), StatusCode::NOT_FOUND, "no subscription was stored");
    tenant.finish().await;
}
