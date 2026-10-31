//! A notification carries every subscribed entity when the subscription defines no query or geoquery (clause 5.8.6).

use crate::{
    building_payloads::building_simple_attributes,
    context_information::subscription::subscription_notification_behaviour::notification_case::{air_quality_updated, created, subscribed, teardown},
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    query::EntitySelector,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// Pins: A notification with all subscribed Entities will be included if query or geoquery are not defined.
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_03_01_check_that_a_notification_is_sent_with_all_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let as_list = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    created(&as_string, &building).await?;

    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/data").and_then(Value::as_array).map(Vec::len), some(gt(0)));
    expect_that!(body.pointer("/data/0/id"), some(eq(&json!(building.id.to_string()))));
    expect_that!(
        body.pointer("/data/0/airQualityLevel/value"),
        some(any!(eq(&json!(5)), eq(&json!("5")), eq(&json!("5.0"))))
    );
    expect_that!(body.pointer("/data/0/name/value"), some(eq(&json!("Eiffel Tower"))));

    teardown(&broker, &id, &building).await;
    Ok(())
}
