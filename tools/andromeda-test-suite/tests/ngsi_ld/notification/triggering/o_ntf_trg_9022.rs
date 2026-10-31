//! An entity created subscription hears the temporal create and not the append.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A Subscription triggered by `entityCreated` alone is notified once of Create Temporal Evolution of an
/// unknown Entity, and of nothing for a newer observation appended to it
/// Clause: 5.8.1.4, Table 5.2.12-1, 5.6.11.4, 5.6.12.4
/// Recorded: clause 4.3.1 gives the current state to the Core API and the Temporal Evolution to the Temporal API
/// and leaves open whether a Temporal API write reaches the current state; Andromeda keeps the current state in
/// step with the Temporal Evolution, so the Entity a Temporal API write addresses has current state
#[gtest]
#[tokio::test]
async fn o_ntf_trg_9022_an_entity_created_subscription_hears_the_temporal_create_and_not_the_append() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_twn_created_only_id = namespace.subscription_id("TwnCreatedOnly")?;
    let twn_created_only_type = namespace.entity_type("TwnCreatedOnly")?;
    let twn_created_only_id = namespace.urn("TwnCreatedOnly", 1)?;
    let twn_created_only_fence_id = namespace.urn("TwnCreatedOnly", "fence")?;
    let subscription_twn_created_only = Subscription::builder()
        .id(subscription_twn_created_only_id.clone())
        .entities([twn_created_only_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("TwnCreatedOnly")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_twn_created_only).send().await?;
    let twn_created_only_evolution = TemporalEntity::builder()
        .id(twn_created_only_id.clone())
        .entity_type(twn_created_only_type.clone())
        .property("v", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&twn_created_only_evolution).send().await?.created()?;
    let instances = TemporalEntity::builder()
        .id(twn_created_only_id.clone())
        .entity_type(twn_created_only_type.clone())
        .property("v", Property::builder(2).observed_at("2030-01-01T00:00:00Z".parse()?))
        .build()?
        .attributes;
    broker.temporal().add_attributes(&twn_created_only_id, &instances).send().await?;
    let twn_created_only_fence = Entity::builder()
        .id(twn_created_only_fence_id)
        .entity_type(twn_created_only_type.clone())
        .property("fence", format!("fence-{namespace}"))
        .build()?;
    broker.entities().create(&twn_created_only_fence).send().await?.done()?;
    // A notification owed before the fence write is delivered before the fence's, so once the fence's has arrived the journal holds every notification the earlier writes owed.
    let selected = [BodyPattern::Contains(format!("fence-{namespace}"))];
    sink.wait_for_at_least_where("TwnCreatedOnly", &selected, 1).await?;
    let selected_2 = [BodyPattern::DoesNotContain(format!("fence-{namespace}"))];
    let notifications = sink.received_where("TwnCreatedOnly", &selected_2).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), nth(0, has_id(&twn_created_only_id)));
    broker.subscriptions().delete(&subscription_twn_created_only_id).send().await?;
    Ok(())
}
