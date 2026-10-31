//! An empty partial update notifies no subscriber.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_pattern::BodyPattern,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
    system_time::{self, SystemMember},
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{AttributeMembers, AttributeName, Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Partial Attribute Update with the empty fragment {} answers 204, leaves the entity and every modifiedAt as
/// they were, and notifies no Subscription to the entity
/// Clause: 5.4, 5.5.8, 5.6.4.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_att_pat_9001_an_empty_partial_update_notifies_no_subscriber() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let entity_modified = system_time::of_entity(entity, SystemMember::ModifiedAt)?;
    let speed_modified = system_time::of_attribute(entity, &["speed"], SystemMember::ModifiedAt)?;
    let brand_modified = system_time::of_attribute(entity, &["brandName"], SystemMember::ModifiedAt)?;
    let fragment = AttributeMembers::builder().build()?;
    let answer = broker
        .with_responses()
        .entities()
        .update_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, fragment)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::ModifiedAt)?, eq(entity_modified));
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(50)));
    expect_that!(system_time::of_attribute(entity_2, &["speed"], SystemMember::ModifiedAt)?, eq(speed_modified));
    expect_that!(entity_2, has_attribute("brandName", any_attribute().value("Mercedes")));
    expect_that!(
        system_time::of_attribute(entity_2, &["brandName"], SystemMember::ModifiedAt)?,
        eq(brand_modified)
    );
    // A real change is owed a notification; notifications of one subscription are delivered in order, so one sent for an empty write would be in the journal before it.
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &format!("entities/{vehicle_id}/attrs/brandName"),
        r#"{"value": "Barrier"}"#,
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let barrier = sink.wait_for_at_least_where(1, &[BodyPattern::Contains("\"Barrier\"".to_owned())], 1).await?;
    expect_that!(barrier, len(ge(1)));
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
