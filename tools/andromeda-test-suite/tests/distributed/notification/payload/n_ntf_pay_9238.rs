//! A show-changes subscription places a copy asking for previous values and relays them.

use andromeda_test_suite::{
    attribute_matcher::{property, relationship},
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    core_context::core_context,
    entity_matcher::has_attribute,
    forwarded_body,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_body, placed_copy, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, Lossless, NgsiLdMediaType, Property, Relationship},
    query::EntitySplitting,
    subscription::{ChangeReporting, Endpoint, NotificationParams, Subscription},
};

/// Pins: A subscription with `showChanges` places a copy on the source whose notification member keeps `showChanges`
/// true, and the previousValue of speed and the previousObject of a deleted owner the source notifies are relayed to
/// the subscriber
/// Clause: 5.8.1.4, 5.8.6, Table 5.2.14-1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9238_a_show_changes_subscription_places_a_copy_asking_for_previous_values_and_relays_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .split_entities(EntitySplitting::Whole)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .show_changes(ChangeReporting::WithPrevious)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy = placed_copy(&source, "s1", &vehicle_type).await?;
    let placed = forwarded_body::subscription(&copy)?;
    let copy_id = placed.id().cloned().or_fail()?;
    expect_that!(placed.notification().show_changes(), eq(ChangeReporting::WithPrevious));
    let person_id = namespace.urn("Person", "a")?;
    let changed = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .output_attribute("speed", Property::output_builder(50).previous_value(40))
        .output_attribute("owner", Relationship::deleted().previous_object(person_id.clone()))
        .build()?;
    let body = notification_body(&namespace, &copy_id, vec![changed], core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let relayed = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects(format!("$.data[?(@.id == '{vehicle_id}')]").parse()?)], 1)
        .await?;
    expect_that!(relayed, len(eq(1)));
    let notification = relayed.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&namespace.subscription_id(1)?));
    let item = notification.data().as_slice().first().or_fail()?;
    expect_that!(item, has_attribute("speed", property().value(50).previous_value(40)));
    expect_that!(item, has_attribute("owner", relationship().null_value().previous_object(person_id)));
    Ok(())
}
