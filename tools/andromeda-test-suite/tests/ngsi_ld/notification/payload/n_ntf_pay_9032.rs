//! Each conformance applies only the fallbacks below it.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals},
    broker,
    conformance_vehicle::vehicle,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{ConformanceVersion, EntityFragment, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Subscriptions with `ngsildConformance` 1.3, 1.4 and 1.8 on one Entity are notified with only the fallbacks
/// of members and datatypes introduced after their version
/// Clause: 4.3.6.8, Table 5.2.12-1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9032_each_conformance_applies_only_the_fallbacks_below_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "thresholds")?;
    let versions = [(3, "thresholds-1-3"), (4, "thresholds-1-4"), (8, "thresholds-1-8")];
    broker.entities().create(&vehicle(&namespace, "thresholds")?).send().await?.done()?;
    for (minor, name) in &versions {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(name)?)
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
            .ngsild_conformance(ConformanceVersion::new(1, *minor))
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(name)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let fragment = EntityFragment::builder().property("note", "thresholds").build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let notified_1_3 = sink.wait_for_at_least("thresholds-1-3", 1).await?;
    let notified_1_4 = sink.wait_for_at_least("thresholds-1-4", 1).await?;
    let notified_1_8 = sink.wait_for_at_least("thresholds-1-8", 1).await?;
    let body_1_3 = &notified_1_3.first().or_fail()?.body;
    // The conformance amendments write shapes of older versions the model reads as current ones, so the notifications
    // are read as received.
    expect_that!(body_1_3, json_count("$.data[0].type", 2));
    expect_that!(body_1_3, json_count("$.data[0].temperature", 2));
    expect_that!(body_1_3, json_equals("$.data[0].speed.observedAt", "2025-01-01T00:00:00Z"));
    expect_that!(body_1_3, json_equals("$.data[0].speed.unitCode", "CEL"));
    expect_that!(body_1_3, json_absent("$.data[0].scope"));
    expect_that!(body_1_3, json_equals("$.data[0].name.type", "Property"));
    let body_1_4 = &notified_1_4.first().or_fail()?.body;
    expect_that!(body_1_4, json_equals("$.data[0].scope", "/conformance"));
    expect_that!(body_1_4, json_equals("$.data[0].name.type", "LanguageProperty"));
    expect_that!(body_1_4, json_equals("$.data[0].category.type", "Property"));
    expect_that!(body_1_4, json_equals("$.data[0].config.type", "Property"));
    expect_that!(body_1_4, json_equals("$.data[0].tags.type", "Property"));
    expect_that!(body_1_4, json_equals("$.data[0].refRooms.type", "Relationship"));
    // Table 4.3.6.8-3 types the `object` of a Relationship as String or String[], so every target is kept in list order.
    expect_that!(
        body_1_4,
        json_equals(
            "$.data[0].refRooms.object",
            vec![namespace.urn_text("Room", "thresholds"), namespace.urn_text("Room", "thresholds-second")]
        )
    );
    expect_that!(body_1_4, json_absent("$.data[0].refBuilding.objectType"));
    let body_1_8 = &notified_1_8.first().or_fail()?.body;
    expect_that!(body_1_8, json_equals("$.data[0].category.type", "VocabProperty"));
    expect_that!(body_1_8, json_equals("$.data[0].config.type", "JsonProperty"));
    expect_that!(body_1_8, json_equals("$.data[0].tags.type", "ListProperty"));
    expect_that!(body_1_8, json_equals("$.data[0].refRooms.type", "ListRelationship"));
    expect_that!(body_1_8, json_equals("$.data[0].refBuilding.objectType", "Building"));
    expect_that!(body_1_8, json_absent("$.data[0].expiresAt"));
    expect_that!(body_1_8, json_absent("$.data[0].speed.valueType"));
    for (_, name) in &versions {
        broker.subscriptions().delete(&namespace.subscription_id(name)?).send().await?;
    }
    Ok(())
}
