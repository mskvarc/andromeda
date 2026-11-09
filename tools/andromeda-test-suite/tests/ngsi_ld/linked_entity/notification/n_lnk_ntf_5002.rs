//! A subscription q with a hop in the grammar form notifies the matching root.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    linked_entity::linked_entities,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson, Representation},
    query::{Join, q},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription whose q is a Linked Entity relation in the grammar's form
/// (isParkedIn{category}=="commercial") is created and notifies the Entity whose Linked Entity satisfies it, with
/// and without a notification join, and does not notify an Entity whose Linked Entity does not; the form with the
/// comparison inside the braces is refused at creation
/// Clause: 4.9, 5.2.12, 5.8.1.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_5002_a_subscription_q_with_a_hop_in_the_grammar_form_notifies_the_matching_root() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_commercial_id = namespace.urn("Building", "commercial")?;
    let building_type = namespace.entity_type("Building")?;
    let building_residential_id = namespace.urn("Building", "residential")?;
    let subscription_bad_id = namespace.subscription_id("bad")?;
    let car_type = namespace.entity_type("Car")?;
    let subscription_joined_id = namespace.subscription_id("joined")?;
    let subscription_plain_id = namespace.subscription_id("plain")?;
    let car_2_id = namespace.urn("Car", 2)?;
    let car_id = namespace.urn("Car", 1)?;
    let entities = [
        Entity::builder()
            .id(building_commercial_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(building_residential_id.clone())
            .entity_type(building_type.clone())
            .property("category", "residential")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let subscription_bad = Subscription::builder()
        .id(subscription_bad_id)
        .entities([car_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("bad")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q: RawJson = r#""isParkedIn{category==\"commercial\"}""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_bad.render())?.set_member("q", &q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_bad,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_joined = Subscription::builder()
        .id(subscription_joined_id)
        .entities([car_type.as_str()])
        .q(q::attr("isParkedIn").linked("category").eq("commercial").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("joined")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .join(Join::Inline)
                .join_level(1)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_joined).send().await?;
    let subscription_plain = Subscription::builder()
        .id(subscription_plain_id)
        .entities([car_type.as_str()])
        .q(q::attr("isParkedIn").linked("category").eq("commercial").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("plain")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_plain).send().await?;
    // the Car parked at the residential Building first, so a notification for it would precede the matching one
    let car_2 = Entity::builder()
        .id(car_2_id)
        .entity_type(car_type.clone())
        .relationship("isParkedIn", building_residential_id.clone())
        .build()?;
    broker.entities().create(&car_2).send().await?.done()?;
    let car = Entity::builder()
        .id(car_id.clone())
        .entity_type(car_type.clone())
        .relationship("isParkedIn", building_commercial_id.clone())
        .build()?;
    broker.entities().create(&car).send().await?.done()?;
    sink.wait_for_at_least("joined", 1).await?;
    let notifications = sink.received("joined").await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.data().as_slice(), len(eq(1)));
    let item = notification.data().first().or_fail()?;
    expect_that!(linked_entities(item, &["isParkedIn"]), nth(0, has_id(&building_commercial_id)));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&car_id)));
    sink.wait_for_at_least("plain", 1).await?;
    let notifications_2 = sink.received("plain").await?;
    expect_that!(notifications_2, len(eq(1)));
    let notification_2 = notifications_2.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification_2.data().as_slice(), len(eq(1)));
    expect_that!(notification_2.data().as_slice(), nth(0, has_id(&car_id)));
    Ok(())
}
