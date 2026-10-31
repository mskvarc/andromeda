//! A relayed entity is notified with the members of a local one.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    context_source::ContextSource,
    core_context::core_context,
    entity_matcher::{has_attribute, lacks_attribute},
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
    relayed_copy::{notification_body, placed_copy_id, register_copy_source, relay},
    type_matcher::has_type,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, Lossless, NgsiLdMediaType, Relationship},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: With notification attributes speed and owner, an entity created on this broker and an entity relayed from
/// the source, both also holding colour, are each notified with id, type, speed and owner and without colour
/// Clause: 5.8.6, 5.3.1
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6808_a_relayed_entity_is_notified_with_the_members_of_a_local_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let local_id = namespace.urn("Vehicle", "local")?;
    let remote_id = namespace.urn("Vehicle", "remote")?;
    let person_id = namespace.urn("Person", "ada")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .attributes(["speed", "owner"])
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let local = Entity::builder()
        .id(local_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .relationship("owner", Relationship::builder(person_id.clone()))
        .property("colour", "red")
        .build()?;
    broker.entities().create(&local).send().await?.done()?;
    let notifications = sink.wait_for_at_least_where(1, &[BodyPattern::Contains(local_id.to_string())], 1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&local_id)));
    let notified = notification.data().first().or_fail()?;
    expect_that!(notified, has_type(&vehicle_type));
    expect_that!(notified, has_attribute("speed", any_attribute().value(40)));
    expect_that!(notified, has_attribute("owner", any_attribute().object(person_id.clone())));
    expect_that!(notified, lacks_attribute("colour"));
    let remote = Entity::output_builder()
        .id(remote_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 40)
        .relationship("owner", Relationship::builder(person_id.clone()))
        .property("colour", "red")
        .build()?;
    let body = notification_body(&namespace, &copy_id, vec![remote], core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications_2 = sink.wait_for_at_least_where(1, &[BodyPattern::Contains(remote_id.to_string())], 1).await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&remote_id)));
    let notified_2 = notification_2.data().first().or_fail()?;
    expect_that!(notified_2, has_type(&vehicle_type));
    expect_that!(notified_2, has_attribute("speed", any_attribute().value(40)));
    expect_that!(notified_2, has_attribute("owner", any_attribute().object(person_id)));
    expect_that!(notified_2, lacks_attribute("colour"));
    Ok(())
}
