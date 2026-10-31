//! A notification carries the empty array.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::{attribute_members, has_attribute},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Representation, Value},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Attributes setting a watched Property to the empty array notifies the Subscription with the
/// Property holding exactly its type and the empty array
/// Clause: 5.8.6, 5.6.2.4, 3.1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9009_a_notification_carries_the_empty_array() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let probe_type = namespace.entity_type("EmptyProbe")?;
    let probe_id = namespace.urn("EmptyProbe", 1)?;
    let probe = Entity::builder()
        .id(probe_id.clone())
        .entity_type(probe_type.clone())
        .property("tags", Value::Array(vec![Value::from("a"), Value::from("b")]))
        .build()?;
    broker.entities().create(&probe).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&probe_type).build()?).with_ids(probe_id.clone())])
        .watched_attributes(["tags"])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("tags", Value::Array(Vec::new())).build()?;
    broker.entities().update_attributes(&probe_id, &fragment).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity, has_attribute("tags", property().value(Value::Array(Vec::new()))));
    expect_that!(attribute_members(entity, "tags"), some(eq(2)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
