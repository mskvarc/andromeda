//! An empty partial attribute update changes and notifies nothing.

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
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::{Entity, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Partial Attribute Update of speed with the empty fragment {} answers 204, leaves the Entity and every
/// `modifiedAt` as they were, and notifies no Subscription to the Entity
/// Clause: 5.4, 5.6.4.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_cmn_val_9030_an_empty_partial_attribute_update_changes_and_notifies_nothing() -> Result<()> {
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
    // The empty fragment reads in the concise form as a Property holding an empty object, which is not what the
    // test sends, so the bytes are sent as written.
    let request = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/speed"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(b"{}"));
    let answer = broker.send_unchecked(request).await;
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
    // A real change is owed a notification; notifications of one subscription are delivered in order, so one sent
    // for the empty write would be in the journal before it. The concise form of a value-only attribute is the
    // bare value, so the fragment is sent as written.
    let request_2 = request_to(&broker, Method::PATCH, &format!("entities/{vehicle_id}/attrs/brandName"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(br#"{"value": "Barrier"}"#));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    sink.wait_for_at_least_where(1, &[BodyPattern::Contains("\"Barrier\"".to_owned())], 1).await?;
    expect_that!(sink.received(1).await?, len(eq(1)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
