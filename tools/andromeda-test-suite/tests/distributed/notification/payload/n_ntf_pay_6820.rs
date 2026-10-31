//! A relayed part without a deletion takes the more recent instance of each attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    core_context::core_context,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_body, placed_copy_id, register_copy_source, register_numbered_copy_source, relay},
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, Lossless, NgsiLdMediaType, Property},
    query::EntitySplitting,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Under a subscription with `splitEntities`, a relayed part with speed 40 and temp 20 both observed at 12:00
/// of an entity another holder answers with speed 55 observed at 13:00, temp 18 observed at 11:00 and fuel 70 is
/// notified with speed 55, temp 20 and fuel 70
/// Clause: 5.8.6, 4.5.5.3
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6820_a_part_with_no_deletion_takes_the_more_recent_instance_of_each_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let split_id = namespace.urn("Vehicle", "split")?;
    let other_holder = Entity::output_builder()
        .id(split_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(55).observed_at("2026-10-06T13:00:00Z".parse()?))
        .property("temp", Property::builder(18).observed_at("2026-10-06T11:00:00Z".parse()?))
        .property("fuel", 70)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s2/ngsi-ld/v1/entities/{split_id}"),
            source_answer::entity(&other_holder),
        )
        .await?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    register_numbered_copy_source(&broker, &source, &namespace, "s2", 2).await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .split_entities(EntitySplitting::Split)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let relayed = Entity::output_builder()
        .id(split_id)
        .entity_type(vehicle_type)
        .property("speed", Property::builder(40).observed_at("2026-10-06T12:00:00Z".parse()?))
        .property("temp", Property::builder(20).observed_at("2026-10-06T12:00:00Z".parse()?))
        .build()?;
    let body = notification_body(&namespace, &copy_id, vec![relayed], core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let completed = sink
        .wait_for_at_least_where(1, &[BodyPattern::Selects("$.data[?(@.fuel)]".parse()?)], 1)
        .await?;
    expect_that!(completed, len(eq(1)));
    let notification = completed.first().or_fail()?.entities(Lossless::Normalized)?;
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    expect_that!(entity, has_attribute("temp", any_attribute().value(20)));
    expect_that!(entity, has_attribute("fuel", any_attribute().value(70)));
    Ok(())
}
