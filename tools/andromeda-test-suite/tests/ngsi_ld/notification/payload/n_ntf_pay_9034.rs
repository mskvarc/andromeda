//! A key values notification is amended in the simplified representation.

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
    model::{ConformanceVersion, EntityFragment, NgsiLdMediaType, Representation, SimplifiedSpelling},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with format `keyValues` and `ngsildConformance` 1.0 is notified with the language map,
/// vocab and JSON value bare and the default instance alone, where the `keyValues` control keeps their wrappers and
/// the dataset map
/// Clause: 4.3.6.8, 4.5.4, Table 5.2.12-1, 5.8.6
///
/// Each amended Subscription has a control Subscription that declares no `ngsildConformance` and is otherwise the
/// same, so every amendment asserted is a difference the control does not show.
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9034_a_key_values_notification_is_amended_in_the_simplified_representation() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "key-values")?;
    let amended_id = namespace.subscription_id("key-values-amended")?;
    let control_id = namespace.subscription_id("key-values-control")?;
    broker.entities().create(&vehicle(&namespace, "key-values")?).send().await?.done()?;
    for (id, endpoint, conformance) in [
        (&amended_id, "key-values-amended", Some(ConformanceVersion::new(1, 0))),
        (&control_id, "key-values-control", None),
    ] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
            .maybe_ngsild_conformance(conformance)
            .notification(
                NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?)
                    .format(Representation::Simplified)
                    .simplified_spelling(SimplifiedSpelling::KeyValues)
                    .build()?,
            )
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let fragment = EntityFragment::builder().property("note", "key-values").build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let controls = sink.wait_for_at_least("key-values-control", 1).await?;
    let amendments = sink.wait_for_at_least("key-values-amended", 1).await?;
    // The conformance amendments write shapes of older versions the model reads as current ones, so the notifications
    // are read as received.
    let control = &controls.first().or_fail()?.body;
    expect_that!(control, json_absent("$.data[0].name.type"));
    expect_that!(control, json_equals("$.data[0].name.languageMap.en", "Tower"));
    expect_that!(control, json_equals("$.data[0].name.languageMap.de", "Turm"));
    expect_that!(control, json_absent("$.data[0].category.type"));
    expect_that!(control, json_equals("$.data[0].category.vocab", "commercial"));
    expect_that!(control, json_absent("$.data[0].config.type"));
    expect_that!(control, json_equals("$.data[0].config.json.level", 3));
    expect_that!(control, json_absent("$.data[0].temperature.type"));
    expect_that!(control, json_count("$.data[0].temperature.dataset.*", 2));
    expect_that!(
        control,
        json_equals(&format!("$.data[0].temperature.dataset['{}']", namespace.dataset_id("Dataset", "one")?), 25)
    );
    expect_that!(control, json_equals("$.data[0].temperature.dataset['@none']", 30));
    let amended = &amendments.first().or_fail()?.body;
    expect_that!(amended, json_equals("$.data[0].type", vehicle_type.as_str()));
    expect_that!(amended, json_absent("$.data[0].scope"));
    expect_that!(amended, json_absent("$.data[0].expiresAt"));
    expect_that!(amended, json_count("$.data[0].name.*", 2));
    expect_that!(amended, json_equals("$.data[0].name.en", "Tower"));
    expect_that!(amended, json_equals("$.data[0].name.de", "Turm"));
    expect_that!(amended, json_equals("$.data[0].category", "commercial"));
    expect_that!(amended, json_absent("$.data[0].config.type"));
    expect_that!(amended, json_equals("$.data[0].config.level", 3));
    expect_that!(amended, json_equals("$.data[0].temperature", 30));
    expect_that!(amended, json_equals("$.data[0].speed", 20));
    broker.subscriptions().delete(&control_id).send().await?;
    broker.subscriptions().delete(&amended_id).send().await?;
    Ok(())
}
