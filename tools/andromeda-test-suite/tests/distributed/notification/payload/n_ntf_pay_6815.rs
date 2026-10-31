//! A relayed entity is notified with its linked entity nested under an inline join.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    core_context::core_context,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
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

/// Pins: A relayed entity located in a city this broker holds is notified under a subscription with join inline as
/// one entity with the city nested under its `locatedIn`
/// Clause: 5.8.6, 4.5.23
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6815_a_relayed_entity_is_notified_with_its_linked_entity_nested_under_an_inline_join() -> Result<()> {
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
                .join(Join::Inline)
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
    expect_that!(notification.data(), len(eq(1)));
    let notified = notification.data().first().or_fail()?;
    expect_that!(
        linked_entities(notified, &["locatedIn"]),
        nth(0, all!(has_id(&city_id), has_attribute("name", any_attribute().value("Paris"))))
    );
    Ok(())
}
