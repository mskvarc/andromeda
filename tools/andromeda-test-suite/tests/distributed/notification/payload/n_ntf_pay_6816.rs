//! A relayed entity is notified with its linked entity beside it under a flat join.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    core_context::core_context,
    entity_lookup::with_id,
    entity_matcher::has_attribute,
    linked_entity::linked_entities,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
    relayed_copy::{notification_body, placed_copy_id, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, Lossless, NgsiLdMediaType, Relationship},
    query::Join,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A relayed entity located in a city this broker holds is notified under a subscription with join flat as
/// two entities, the vehicle and the city, the vehicle without a nested entity
/// Clause: 5.8.6, 4.5.23
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6816_a_relayed_entity_is_notified_with_its_linked_entity_beside_it_under_a_flat_join() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "join")?;
    let city_id = namespace.urn("City", 1)?;
    let city = Entity::builder()
        .id(city_id.clone())
        .entity_type(namespace.entity_type("City")?)
        .property("name", "Paris")
        .build()?;
    broker.entities().create(&city).send().await?.done()?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .join(Join::Flat)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let relayed = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .relationship("locatedIn", Relationship::builder(city_id.clone()))
        .build()?;
    let body = notification_body(&namespace, &copy_id, vec![relayed], core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data(), len(eq(2)));
    expect_that!(
        with_id(notification.data(), &city_id),
        some(has_attribute("name", any_attribute().value("Paris")))
    );
    let vehicle = with_id(notification.data(), &vehicle_id).or_fail()?;
    expect_that!(linked_entities(vehicle, &["locatedIn"]), is_empty());
    Ok(())
}
