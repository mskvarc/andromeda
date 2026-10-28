//! ETSI `046_13`: a notification the endpoint cannot receive sets `notification.status` to `failed` and records the instant in
//! `notification.lastFailure` (clause 5.8.6).

use crate::{
    context_information::subscription::subscription_notification_behaviour::notification_case::{
        INSTANT_PATTERN,
        air_quality_updated,
        created,
        subscription_created,
        teardown,
    },
    suite_payloads::building_simple_attributes,
};
use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    eventually::{Poll, delivery_deadline, eventually},
    namespace::Namespace,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: If the response to the notification request is different than 200 OK then implementations shall: Update
/// notification.lastFailure with a timestamp representing the current date and time, update notification.status to "failed"
/// Clause: 5.8.6
/// Changed: Create Subscription sends the material's endpoint `http://localhost:1111/notify` as it is, where nothing listens in
/// the Robot run, so every notification fails; the standing substitution would send it to WireMock, which answers 200, so the
/// port names `{{unreachable_origin}}/notify`, where nothing listens either
/// Changed: Sleep 10s, then one Retrieve Subscription, becomes that Retrieve Subscription repeated every 250 ms until the delivery deadline
/// until its assertions hold: the case waits for the broker to record the failed notification, which the retrieval itself observes
#[gtest]
#[tokio::test]
async fn e046_13_01_check_that_last_failure_and_status_are_updated_if_a_notification_could_not_be_sent() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let notify = SuiteConfig::get()?.unreachable_origin().join("notify")?;
    let endpoint = Endpoint::builder(notify.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .build()?;
    let _subscription = subscription_created(&as_list, &subscription).await?;
    created(&as_string, &building).await?;
    air_quality_updated(&as_list, &building).await?;

    let retrieval = broker.with_context(vocabulary.context())?.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let failed = eventually(delivery_deadline(), || async {
        match retrieval.with_responses().subscriptions().retrieve(&id).send().await {
            Ok(received) => match json_body(&received) {
                Ok(body) if body.pointer("/notification/status") == Some(&json!("failed")) && body.pointer("/notification/lastFailure").is_some() => {
                    Poll::Ready(body)
                }
                Ok(body) => Poll::Pending(body.to_string()),
                Err(error) => Poll::Pending(error.to_string()),
            },
            Err(error) => Poll::Pending(error.to_string()),
        }
    })
    .await?;
    expect_that!(failed.pointer("/notification/status"), some(eq(&json!("failed"))));
    expect_that!(
        failed.pointer("/notification/lastFailure").and_then(Value::as_str),
        some(matches_regex(INSTANT_PATTERN))
    );

    teardown(&broker, &id, &building).await;
    Ok(())
}
