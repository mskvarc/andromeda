//! The structure of a notification, the attributes it carries and the expansion of the URIs it is written with (clauses
//! 5.8.6 and 5.3).

use crate::{
    building_payloads::building_simple_attributes,
    context_information::subscription::subscription_notification_behaviour::notification_case::{
        INSTANT_PATTERN,
        air_quality_updated,
        created,
        subscribed,
        teardown,
    },
};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, vocabulary::Vocabulary};
use googletest::prelude::*;
use iri_rs::IriBuf;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Context, NgsiLdMediaType, Representation},
    query::EntitySelector,
    subscription::{Activation, Endpoint, NotificationParams, Subscription, SubscriptionId},
};
use serde_json::{Value, json};

/// Checks the members every notification carries: its `type`, the `subscriptionId` and the `notifiedAt` instant.
fn expect_envelope(body: &Value, id: &SubscriptionId) {
    expect_that!(body.pointer("/type"), some(eq(&json!("Notification"))));
    expect_that!(body.pointer("/subscriptionId"), some(eq(&json!(id.to_string()))));
    expect_that!(body.pointer("/notifiedAt").and_then(Value::as_str), some(matches_regex(INSTANT_PATTERN)));
}

/// Pins: The structure of the notification message shall be as mandated by clause 5.3. Valid notification with attributes
/// as stated above
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_07_01_check_notification_structure() -> Result<()> {
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
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).format(Representation::Normalized).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_envelope(&body, &id);

    teardown(&broker, &id, &building).await;
    Ok(())
}

/// Pins: The structure of the notification message shall be as mandated by clause 5.3.
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_07_02_check_correct_attributes_are_included() -> Result<()> {
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
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new("Building".parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).attributes(["airQualityLevel"]).build()?)
        .build()?;
    let _subscription = subscribed(&as_list, &subscription).await?;

    air_quality_updated(&as_list, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_envelope(&body, &id);
    expect_that!(body.pointer("/data/0"), some(predicate(Value::is_object)));
    expect_that!(body.pointer("/data/0/name"), none());
    expect_that!(body.pointer("/data/0/airQualityLevel"), some(anything()));

    teardown(&broker, &id, &building).await;
    Ok(())
}

/// Pins: The structure of the notification message shall be as mandated by clause 5.3.
/// Clause: 5.8.6
#[gtest]
#[tokio::test]
async fn e046_07_03_check_uri_expansion_is_observed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory", "airQualityLevel", "almostFull"]).await?;
    let sink = NotificationSink::start().await?;
    let as_string = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let core_context = Context::from(IriBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_owned())?);
    let core = broker.with_context(core_context)?.with_context_delivery(ContextDelivery::LdJsonBody);
    let building = building_simple_attributes(&namespace, 1)?;
    created(&as_string, &building).await?;

    let building_type = namespace.expanded_term("Building")?;
    let air_quality_level = namespace.expanded_term("airQualityLevel")?;
    let id: SubscriptionId = namespace.urn_text("Subscription", 1).parse()?;
    let endpoint = Endpoint::builder(sink.endpoint(1)?.as_str()).accept(NgsiLdMediaType::Json).build()?;
    // The subscription owns its identifier and keeps it private; the teardown deletes it by the same one.
    let subscription = Subscription::builder()
        .id(id.clone())
        .activation(Activation::Active)
        .entities([EntitySelector::new(building_type.parse()?).with_ids(building.id.clone())])
        .notification(NotificationParams::builder(endpoint).attributes([air_quality_level.as_str()]).build()?)
        .build()?;
    let _subscription = subscribed(&core, &subscription).await?;

    air_quality_updated(&core, &building).await?;
    let notification = sink.wait_for(1, 1).await?;
    let body = notification.json()?;
    expect_envelope(&body, &id);
    expect_that!(
        body.pointer("/data/0").and_then(|entity| entity.get(air_quality_level.as_str())),
        some(anything())
    );

    teardown(&broker, &id, &building).await;
    Ok(())
}
