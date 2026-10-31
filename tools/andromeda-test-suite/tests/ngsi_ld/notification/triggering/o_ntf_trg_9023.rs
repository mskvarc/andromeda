//! A newer temporal append notifies the update with its previous value.

use andromeda_test_suite::{
    attribute_matcher::property,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Attribute, AttributeName, Entity, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A Subscription triggered by `attributeUpdated` with `showChanges` is notified once of an observation of v
/// in 2030 appended to the Temporal Evolution of an Entity whose v is 5, carrying v 200 and `previousValue` 5
/// Clause: 5.8.1.4, 5.8.6, 5.6.12.4, 4.5.5.3
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9023_a_newer_temporal_append_notifies_the_update_with_its_previous_value() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("TwnAppend")?;
    let append_type = namespace.entity_type("TwnAppend")?;
    let append_id = namespace.urn("TwnAppend", 1)?;
    let append = Entity::builder()
        .id(append_id.clone())
        .entity_type(append_type.clone())
        .property("v", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&append).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([append_type.as_str()])
        .notification_triggers([NotificationTrigger::AttributeUpdated])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("TwnAppend")?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let instances = TemporalEntity::builder()
        .id(append_id.clone())
        .entity_type(append_type)
        .property("v", Property::builder(200).observed_at("2030-01-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&append_id, &instances).send().await?;
    let fence = format!("fence-{namespace}");
    let patch = Attribute::from(Property::builder(fence.as_str()).build()?).into_patch();
    broker
        .entities()
        .update_attribute(&append_id, &"v".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    // A notification owed before the fence write is delivered before the fence's, so once the fence's has arrived the
    // journal holds every notification the earlier writes owed.
    sink.wait_for_at_least_where("TwnAppend", &[BodyPattern::Contains(fence.clone())], 1).await?;
    let before_fence = sink.received_where("TwnAppend", &[BodyPattern::DoesNotContain(fence)]).await?;
    expect_that!(before_fence, len(eq(1)));
    let notification = before_fence.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, has_attribute("v", property().value(200).previous_value(5)))
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
