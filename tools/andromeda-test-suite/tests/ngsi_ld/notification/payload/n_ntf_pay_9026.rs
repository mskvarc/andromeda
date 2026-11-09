//! A json notification carries the context in no part of the body.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    header_condition::HeaderCondition,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    model::{Entity, EntityFragment, Lossless, NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: An application/json notification carries the @context in its Link header and as a member of neither the
/// Notification nor the Entity in data
/// Clause: 6.3.5, 6.3.6, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9026_a_json_notification_carries_the_context_in_no_part_of_the_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_json_context_id = namespace.urn("Building", "json-context")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_json_context_id = namespace.subscription_id("json-context")?;
    let building_json_context = Entity::builder()
        .id(building_json_context_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Tour Eiffel")
        .property("category", "office")
        .build()?;
    broker.entities().create(&building_json_context).send().await?.done()?;
    let subscription_json_context = Subscription::builder()
        .id(subscription_json_context_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&building_type).build()?).with_ids(building_json_context_id.clone())])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("json-context")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_json_context).send().await?;
    let fragment = EntityFragment::builder().property("name", "Le Tour Eiffel").build()?;
    broker.entities().append_attributes(&building_json_context_id, &fragment).send().await?.done()?;
    let selected = [BodyPattern::Contains("Le Tour Eiffel".to_owned())];
    sink.wait_for_at_least_where("json-context", &selected, 1).await?;
    let selected_2 = [BodyPattern::Contains("Le Tour Eiffel".to_owned())];
    let fields = [HeaderCondition::Contains(HeaderName::from_static("link"), "json-ld#context".to_owned())];
    let notifications = sink.received_matching("json-context", &selected_2, &fields).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.context(), none());
    let item = notification.data().first().or_fail()?;
    expect_that!(item.context.as_ref(), none());
    broker.subscriptions().delete(&subscription_json_context_id).send().await?;
    Ok(())
}
