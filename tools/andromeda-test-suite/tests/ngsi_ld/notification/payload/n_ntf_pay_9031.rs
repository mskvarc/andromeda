//! A 1 0 conformance notifies the 1 0 shape.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals, json_matches},
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

/// Pins: A Subscription with `ngsildConformance` 1.0 is notified of the first type alone, without scope or
/// `expiresAt`, with each 1.8 and 1.4 datatype reformatted as a Property or Relationship, without `observedAt`,
/// `unitCode`, `valueType` or `objectType`, and with the default instance alone, where the control keeps all of
/// them
/// Clause: 4.3.6.8, Table 5.2.12-1, 5.8.6
///
/// Each amended Subscription has a control Subscription that declares no `ngsildConformance` and is otherwise the
/// same, so every amendment asserted is a difference the control does not show.
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9031_a_1_0_conformance_notifies_the_1_0_shape() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "one-zero")?;
    let amended_id = namespace.subscription_id("one-zero-amended")?;
    let control_id = namespace.subscription_id("one-zero-control")?;
    broker.entities().create(&vehicle(&namespace, "one-zero")?).send().await?.done()?;
    for (id, endpoint, conformance) in [
        (&amended_id, "one-zero-amended", Some(ConformanceVersion::new(1, 0))),
        (&control_id, "one-zero-control", None),
    ] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
            .maybe_ngsild_conformance(conformance)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let fragment = EntityFragment::builder().property("note", "one-zero").build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let controls = sink.wait_for_at_least("one-zero-control", 1).await?;
    let amendments = sink.wait_for_at_least("one-zero-amended", 1).await?;
    let control = &controls.first().or_fail()?.body;
    // The conformance amendments write shapes of older versions the model reads as current ones, so the notifications
    // are read as received.
    expect_that!(control, json_count("$.data[0].type", 2));
    expect_that!(control, json_equals("$.data[0].type[0]", namespace.entity_type("Vehicle")?.to_string()));
    expect_that!(control, json_equals("$.data[0].type[1]", namespace.entity_type("Car")?.to_string()));
    expect_that!(control, json_equals("$.data[0].scope", "/conformance"));
    expect_that!(control, json_matches("$.data[0].expiresAt", ".")?);
    expect_that!(control, json_matches("$.data[0].speed.observedAt", ".")?);
    expect_that!(control, json_equals("$.data[0].speed.unitCode", "CEL"));
    expect_that!(control, json_equals("$.data[0].name.type", "LanguageProperty"));
    expect_that!(control, json_equals("$.data[0].refRooms.type", "ListRelationship"));
    expect_that!(control, json_equals("$.data[0].refBuilding.objectType", "Building"));
    expect_that!(control, json_count("$.data[0].temperature", 2));
    let amended = &amendments.first().or_fail()?.body;
    expect_that!(amended, json_equals("$.data[0].type", vehicle_type.as_str()));
    expect_that!(amended, json_absent("$.data[0].scope"));
    expect_that!(amended, json_absent("$.data[0].expiresAt"));
    expect_that!(amended, json_count("$.data[0].speed.*", 2));
    expect_that!(amended, json_equals("$.data[0].speed.type", "Property"));
    expect_that!(amended, json_equals("$.data[0].speed.value", 20));
    expect_that!(amended, json_count("$.data[0].name.*", 2));
    expect_that!(amended, json_equals("$.data[0].name.type", "Property"));
    expect_that!(amended, json_equals("$.data[0].name.value.en", "Tower"));
    expect_that!(amended, json_equals("$.data[0].name.value.de", "Turm"));
    expect_that!(amended, json_count("$.data[0].name.value.*", 2));
    expect_that!(amended, json_count("$.data[0].category.*", 2));
    expect_that!(amended, json_equals("$.data[0].category.type", "Property"));
    expect_that!(amended, json_equals("$.data[0].category.value", "commercial"));
    expect_that!(amended, json_count("$.data[0].config.*", 2));
    expect_that!(amended, json_equals("$.data[0].config.type", "Property"));
    expect_that!(amended, json_equals("$.data[0].config.value.level", 3));
    expect_that!(amended, json_absent("$.data[0].config.value.type"));
    expect_that!(amended, json_count("$.data[0].tags.*", 2));
    expect_that!(amended, json_equals("$.data[0].tags.type", "Property"));
    expect_that!(amended, json_count("$.data[0].tags.value", 2));
    expect_that!(amended, json_equals("$.data[0].tags.value[0]", "a"));
    expect_that!(amended, json_equals("$.data[0].tags.value[1]", "b"));
    expect_that!(amended, json_count("$.data[0].refRooms.*", 2));
    expect_that!(amended, json_equals("$.data[0].refRooms.type", "Relationship"));
    // Table 4.3.6.8-3 types the `object` of a Relationship as String or String[], so every target is kept in list order.
    expect_that!(
        amended,
        json_equals(
            "$.data[0].refRooms.object",
            vec![namespace.urn_text("Room", "one-zero"), namespace.urn_text("Room", "one-zero-second")]
        )
    );
    expect_that!(amended, json_count("$.data[0].refBuilding.*", 2));
    expect_that!(amended, json_equals("$.data[0].refBuilding.type", "Relationship"));
    expect_that!(amended, json_equals("$.data[0].refBuilding.object", namespace.urn_text("Building", "one-zero")));
    expect_that!(amended, json_count("$.data[0].temperature.*", 2));
    expect_that!(amended, json_equals("$.data[0].temperature.type", "Property"));
    expect_that!(amended, json_equals("$.data[0].temperature.value", 30));
    broker.subscriptions().delete(&control_id).send().await?;
    broker.subscriptions().delete(&amended_id).send().await?;
    Ok(())
}
