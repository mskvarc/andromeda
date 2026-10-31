//! The steps the registration notification tests share: the context source registration that is created, the check of
//! the `ContextSourceNotification` it is notified in, and the teardown.

use crate::{
    entity_operations::ld_json_body_client,
    geometry_payloads::point,
    registration_operations::csource_endpoint,
    registration_payloads::building_registration,
    registration_subscription_operations::{create_registration_subscription, without_members},
};
use andromeda_test_suite::{
    broker,
    broker::BrokerClient,
    cleanup::Cleanup,
    comparison::matches_document,
    namespace::Namespace,
    ngsild_date::is_ngsild_date,
    notification_sink::NotificationSink,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::Input,
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
    subscription::{RegistrationSubscription, Subscription, SubscriptionId},
};
use serde_json::{Value, json};
use url::Url;

/// `Create Context Source Registration`: registers `registration` with the vocabulary as a one-element `@context` list
/// in the body, requiring 201; answers the cleanup that deletes it.
pub(crate) async fn registered(broker: &BrokerClient, vocabulary: &Vocabulary, registration: &CSourceRegistration<Input>) -> Result<Cleanup> {
    let cleanup = Cleanup::of(broker).registration_of(registration);
    ld_json_body_client(broker, vocabulary)?.registrations().register(registration).send().await?;
    Ok(cleanup)
}

/// Registers `registration` as [`registered`] does, whatever the answer; answers the cleanup that deletes it.
pub(crate) async fn registered_any(broker: &BrokerClient, vocabulary: &Vocabulary, registration: &CSourceRegistration<Input>) -> Result<Cleanup> {
    let cleanup = Cleanup::of(broker).registration_of(registration);
    ld_json_body_client(broker, vocabulary)?
        .registrations()
        .register(registration)
        .send()
        .await
        .any_answer();
    Ok(cleanup)
}

/// Requires `body` to be the `ContextSourceNotification` of `subscription` notified at a date, carrying the
/// registration `registration` alone, for `reason`.
pub(crate) fn expect_csource_notification(body: &Value, subscription: &SubscriptionId, registration: &RegistrationId, reason: &str) {
    expect_that!(body.pointer("/type"), some(eq(&json!("ContextSourceNotification"))));
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(subscription.to_string()))));
    expect_that!(body.pointer("/notifiedAt").and_then(Value::as_str).map(is_ngsild_date), some(eq(true)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(registration.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/triggerReason"), some(eq(&json!(reason))));
}

/// The terms of the `@context` the Building source tests register.
pub(crate) const BUILDING_SOURCE_TERMS: [&str; 4] = ["Building", "name", "subCategory", "locatedAt"];

/// The cases that subscribe first and then register a source: registers `terms`, creates `subscription` notifying
/// endpoint 1 of `sink` (201), registers the registration `source` makes for the context source
/// `sources/<ns>/my.csource.org:1026` whatever the answer, requires its newly matching notification within 5 s, and
/// tears both down.
pub(crate) async fn notified_of_source(
    namespace: &Namespace,
    sink: &NotificationSink,
    terms: &[&str],
    subscription: &Subscription<Input, RegistrationSubscription>,
    source: impl FnOnce(&Url) -> Result<CSourceRegistration<Input>>,
) -> Result<()> {
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, namespace, terms).await?;
    let id = subscription.id().or_fail()?;
    let _subscription = create_registration_subscription(&ld_json_body_client(&broker, &vocabulary)?, subscription, &sink.endpoint(1)?).await?;
    let endpoint = csource_endpoint(namespace, "my.csource.org:1026").await?;
    let registration = source(&endpoint)?;
    let registration_id = registration.id().or_fail()?;
    let _registration = registered_any(&broker, &vocabulary, &registration).await?;

    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_csource_notification(&body, id, registration_id, "newlyMatching");

    teardown(&broker, registration_id, id).await
}

/// [`notified_of_source`] with the terms [`BUILDING_SOURCE_TERMS`] and the [`building_registration`] `-1`.
pub(crate) async fn notified_of_building_source(
    namespace: &Namespace,
    sink: &NotificationSink,
    subscription: &Subscription<Input, RegistrationSubscription>,
) -> Result<()> {
    notified_of_source(namespace, sink, &BUILDING_SOURCE_TERMS, subscription, |endpoint| {
        building_registration(namespace, 1, endpoint)
    })
    .await
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-1` of every `Building` at `endpoint`, located at the
/// point -8.521, 41.2.
pub(crate) fn located_building_registration(namespace: &Namespace, endpoint: &Url) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .location(point(-8.521, 41.2)?)
        .build()?)
}

/// The members of a retrieved `notification` the delivery cases leave uncompared.
const UNCOMPARED_DELIVERY: [&str; 4] = ["lastNotification", "lastSuccess", "lastFailure", "timesFailed"];

/// Requires the `notification` of the retrieved subscription `body` to hold the keyValues format, an endpoint that
/// accepts `application/json` (its `uri` uncompared), the status `status` and one notification sent, and a non-empty
/// value for each member of `instants`.
pub(crate) fn expect_delivery_state(mut body: Value, status: &str, instants: [&str; 2]) {
    let notification = body.get_mut("notification").map(Value::take).unwrap_or_default();
    for name in instants {
        expect_that!(notification.get(name).is_some_and(is_non_empty), eq(true), "{name} is non-empty");
    }
    let mut notification = without_members(notification, &UNCOMPARED_DELIVERY);
    if let Some(endpoint) = notification.get_mut("endpoint") {
        *endpoint = without_members(endpoint.take(), &["uri"]);
    }
    expect_that!(
        &notification,
        matches_document(json!({
            "format": "keyValues",
            "endpoint": {"accept": "application/json"},
            "status": status,
            "timesSent": 1,
        }))
    );
}

/// Whether `value` is a non-empty list, object or string.
fn is_non_empty(value: &Value) -> bool {
    match value {
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Deletes `registration`, requiring a status below
/// 400, and the subscription `subscription`, whatever the answer.
pub(crate) async fn teardown(broker: &BrokerClient, registration: &RegistrationId, subscription: &SubscriptionId) -> Result<()> {
    broker.registrations().delete(registration).send().await?;
    broker.registration_subscriptions().delete(subscription).send().await.any_answer();
    Ok(())
}
