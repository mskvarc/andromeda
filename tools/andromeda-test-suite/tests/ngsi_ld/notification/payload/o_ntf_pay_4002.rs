//! A notification with pick and omit carries the picked members minus the omitted ones and a member in both is refused.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::CreateSubscription,
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, Representation},
    query::projection::Projection,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose notification carries pick and omit naming different members is accepted and notifies
/// each entity reduced to the picked members minus the omitted ones; a subscription naming a member in both is
/// refused with `BadRequestData`
/// Clause: 4.21, 5.2.14.1, 5.8.1.4, 5.8.6
/// Recorded: Table 5.2.14.1-1 defines pick and omit side by side and the clauses name no error for either
/// combination. Applying both when they name different members, and refusing a member named in both as a
/// self-contradictory projection with `BadRequestData`, is Andromeda's reading, so the file is kind O. Ruled by the
/// user (2026-10-05; the refusal of a member named in both 2026-10-11)
#[gtest]
#[tokio::test]
async fn o_ntf_pay_4002_a_notification_with_pick_and_omit_carries_the_picked_members_minus_the_omitted_ones_and_a_member_in_both_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .pick("id,category".parse::<Projection>()?)
                .omit("name".parse::<Projection>()?)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("category", "commercial")
        .property("name", "Depot")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let notifications = sink.wait_for_at_least(1, 1).await?;
    let notification = notifications.first().or_fail()?.projected_entities(Lossless::Normalized)?;
    let entity = notification.data().first().or_fail()?;
    expect_that!(entity.id, some(eq(&building_id)));
    expect_that!(entity, has_attribute("category", any_attribute().value("commercial")));
    expect_that!(entity, lacks_attribute("name"));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id)
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .pick("id,category,name".parse::<Projection>()?)
                .omit("name".parse::<Projection>()?)
                .build()?,
        )
        .build()?;
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription_2 })?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
