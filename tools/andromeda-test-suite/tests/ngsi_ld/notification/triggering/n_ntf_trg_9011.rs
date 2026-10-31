//! Several dataset instances of one create notify once.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::{has_attribute, has_instances},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Property},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: The creation of an Entity whose attribute holds a default and two `datasetId` instances notifies a
/// Subscription to its type once, carrying all three instances
/// Clause: 4.5.5, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9011_several_dataset_instances_of_one_create_notify_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_np_dataset_id = namespace.subscription_id("np-dataset")?;
    let np_dataset_type = namespace.entity_type("NpDataset")?;
    let np_dataset_id = namespace.urn("NpDataset", 1)?;
    let subscription_np_dataset = Subscription::builder()
        .id(subscription_np_dataset_id.clone())
        .entities([np_dataset_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("np-dataset")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_np_dataset).send().await?;
    let np_dataset = Entity::builder()
        .id(np_dataset_id.clone())
        .entity_type(np_dataset_type.clone())
        .property("speed", 1)
        .property("speed", Property::builder(2).dataset_id(namespace.dataset_id("Dataset", "gps")?))
        .property("speed", Property::builder(3).dataset_id(namespace.dataset_id("Dataset", "radar")?))
        .build()?;
    broker.entities().create(&np_dataset).send().await?.done()?;
    // The barrier update is owed the next notification; notifications of one subscription are delivered in order, so a second notification of the write under test would be in the journal before it.
    let fragment = EntityFragment::builder().property("a", "barrier").build()?;
    broker.entities().update_attributes(&np_dataset_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("\"barrier\"".to_owned())];
    sink.wait_for_at_least_where("np-dataset", &selected, 1).await?;
    let notifications = sink.received("np-dataset").await?;
    expect_that!(notifications, len(eq(2)));
    let notification = notifications.get(1).or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_attribute("a", any_attribute().value("barrier"))));
    let notifications_2 = sink.received("np-dataset").await?;
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_instances("speed", 3)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&np_dataset_id)));
    broker.subscriptions().delete(&subscription_np_dataset_id).send().await?;
    Ok(())
}
