//! A concise notification is amended in the concise representation.

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
    model::{ConformanceVersion, EntityFragment, NgsiLdMediaType, Representation},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with format concise and `ngsildConformance` 1.0 is notified with each attribute in the
/// concise form of its 1.0 fallback, where the concise control keeps `unitCode` and the `languageMap`
/// Clause: 4.3.6.8, 4.5.2.3, Table 5.2.12-1, 5.8.6
///
/// Each amended Subscription has a control Subscription that declares no `ngsildConformance` and is otherwise the
/// same, so every amendment asserted is a difference the control does not show.
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9033_a_concise_notification_is_amended_in_the_concise_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "concise")?;
    let amended_id = namespace.subscription_id("concise-amended")?;
    let control_id = namespace.subscription_id("concise-control")?;
    broker.entities().create(&vehicle(&namespace, "concise")?).send().await?.done()?;
    for (id, endpoint, conformance) in [
        (&amended_id, "concise-amended", Some(ConformanceVersion::new(1, 0))),
        (&control_id, "concise-control", None),
    ] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
            .maybe_ngsild_conformance(conformance)
            .notification(
                NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?)
                    .format(Representation::Concise)
                    .build()?,
            )
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let fragment = EntityFragment::builder().property("note", "concise").build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let controls = sink.wait_for_at_least("concise-control", 1).await?;
    let amendments = sink.wait_for_at_least("concise-amended", 1).await?;
    let control = &controls.first().or_fail()?.body;
    // The conformance amendments write shapes of older versions the model reads as current ones, so the notifications
    // are read as received.
    expect_that!(control, json_equals("$.data[0].speed.unitCode", "CEL"));
    expect_that!(control, json_absent("$.data[0].speed.type"));
    expect_that!(control, json_absent("$.data[0].name.type"));
    expect_that!(control, json_equals("$.data[0].name.languageMap.en", "Tower"));
    expect_that!(control, json_equals("$.data[0].name.languageMap.de", "Turm"));
    let amended = &amendments.first().or_fail()?.body;
    expect_that!(amended, json_equals("$.data[0].type", vehicle_type.as_str()));
    expect_that!(amended, json_absent("$.data[0].scope"));
    expect_that!(amended, json_absent("$.data[0].expiresAt"));
    expect_that!(amended, json_equals("$.data[0].speed", 20));
    expect_that!(amended, json_count("$.data[0].name.*", 2));
    expect_that!(amended, json_equals("$.data[0].name.en", "Tower"));
    expect_that!(amended, json_equals("$.data[0].name.de", "Turm"));
    expect_that!(amended, json_equals("$.data[0].category", "commercial"));
    expect_that!(amended, json_count("$.data[0].tags", 2));
    expect_that!(amended, json_equals("$.data[0].tags[0]", "a"));
    expect_that!(amended, json_equals("$.data[0].tags[1]", "b"));
    expect_that!(amended, json_absent("$.data[0].refBuilding.type"));
    expect_that!(amended, json_equals("$.data[0].refBuilding.object", namespace.urn_text("Building", "concise")));
    expect_that!(amended, json_absent("$.data[0].refRooms.type"));
    // Table 4.3.6.8-3 types the `object` of a Relationship as String or String[], so every target is kept in list order.
    expect_that!(
        amended,
        json_equals(
            "$.data[0].refRooms.object",
            vec![namespace.urn_text("Room", "concise"), namespace.urn_text("Room", "concise-second")]
        )
    );
    expect_that!(amended, json_equals("$.data[0].temperature", 30));
    broker.subscriptions().delete(&control_id).send().await?;
    broker.subscriptions().delete(&amended_id).send().await?;
    Ok(())
}
