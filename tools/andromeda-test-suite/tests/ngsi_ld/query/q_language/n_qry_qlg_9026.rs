//! A dot selects a newline in a query and a subscription.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Attribute, AttributeName, Entity, NgsiLdMediaType, Property},
    query::{Locality, q},
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: A Subscription whose q is name~="Merc.des" is notified of an Entity whose name is Merc, a newline and des,
/// one whose q is name~="^Xerc" is not, and Query Entities with the first pattern selects that Entity
/// Clause: 4.9, 5.8.6; IEEE 1003.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9026_a_dot_selects_a_newline_in_a_query_and_a_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_dot_matching_id = namespace.subscription_id("dot-matching")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_dot_control_id = namespace.subscription_id("dot-control")?;
    let building_dot_newline_id = namespace.urn("Building", "dot-newline")?;
    let subscription_dot_matching = Subscription::builder()
        .id(subscription_dot_matching_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("name").matches("\"Merc.des\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("dot-matching")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_dot_matching).send().await?;
    let subscription_dot_control = Subscription::builder()
        .id(subscription_dot_control_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated, NotificationTrigger::AttributeUpdated])
        .q(q::attr("name").matches("\"^Xerc\"").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("dot-control")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_dot_control).send().await?;
    let building_dot_newline = Entity::builder()
        .id(building_dot_newline_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Merc\ndes")
        .build()?;
    broker.entities().create(&building_dot_newline).send().await?.done()?;
    let selected = [BodyPattern::Contains(format!("urn:ngsi-ld:Building:{namespace}-dot-newline"))];
    sink.wait_for_at_least_where("dot-matching", &selected, 1).await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ids(vec![building_dot_newline_id.clone()])
        .q(q::attr("name").matches("\"Merc.des\""))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_dot_newline_id)));
    // The barrier update is owed the next notification of the control; notifications of one subscription are delivered in order, so a notification of the creation would be in the journal before it.
    let patch = Attribute::from(Property::builder("Xerces").build()?).into_patch();
    broker
        .entities()
        .update_attribute(&building_dot_newline_id, &"name".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .done()?;
    let selected_2 = [BodyPattern::Contains("Xerces".to_owned())];
    sink.wait_for_at_least_where("dot-control", &selected_2, 1).await?;
    let notifications = sink.received("dot-control").await?;
    expect_that!(notifications, len(eq(1)));
    broker.subscriptions().delete(&subscription_dot_matching_id).send().await?;
    broker.subscriptions().delete(&subscription_dot_control_id).send().await?;
    Ok(())
}
