//! An undeclared conformance leaves the notification unchanged.

use andromeda_test_suite::{
    body_json::{json_count, json_equals, json_matches},
    broker,
    conformance_vehicle::vehicle,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription without `ngsildConformance` is notified with both types, the scope, `expiresAt`, the
/// `LanguageProperty`, `VocabProperty` and `ListRelationship`, `objectType`, both instances and `unitCode`
/// Clause: Table 5.2.12-1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9035_an_undeclared_conformance_leaves_the_notification_unchanged() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "absent")?;
    let subscription_id = namespace.subscription_id("absent")?;
    broker.entities().create(&vehicle(&namespace, "absent")?).send().await?.done()?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("absent")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = EntityFragment::builder().property("note", "absent").build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let notifications = sink.wait_for_at_least("absent", 1).await?;
    let body = &notifications.first().or_fail()?.body;
    // The control of the conformance amendments is read as received, as its amended siblings are.
    expect_that!(body, json_count("$.data[0].type", 2));
    expect_that!(body, json_equals("$.data[0].scope", "/conformance"));
    expect_that!(body, json_matches("$.data[0].expiresAt", ".")?);
    expect_that!(body, json_equals("$.data[0].name.type", "LanguageProperty"));
    expect_that!(body, json_equals("$.data[0].category.type", "VocabProperty"));
    expect_that!(body, json_equals("$.data[0].refRooms.type", "ListRelationship"));
    expect_that!(body, json_equals("$.data[0].refBuilding.objectType", "Building"));
    expect_that!(body, json_count("$.data[0].temperature", 2));
    expect_that!(body, json_equals("$.data[0].speed.unitCode", "CEL"));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
