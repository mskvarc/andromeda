//! Show changes under a 1 3 conformance gives the previous value of the property.

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
    model::{ConformanceVersion, EntityFragment, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{ChangeReporting, Endpoint, NotificationParams, Subscription},
};

/// Pins: Updating a `LanguageProperty` notifies a Subscription with `showChanges` and `ngsildConformance` 1.3 with
/// a Property whose value and `previousValue` are the new and old language maps and no `languageMap` or
/// `previousLanguageMap`, where the control carries `previousLanguageMap`
/// Clause: 4.3.6.8, 4.5.2, 5.8.6
///
/// Each amended Subscription has a control Subscription that declares no `ngsildConformance` and is otherwise the
/// same, so every amendment asserted is a difference the control does not show.
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9037_show_changes_under_a_1_3_conformance_gives_the_previous_value_of_the_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", "show-changes")?;
    let amended_id = namespace.subscription_id("show-changes-amended")?;
    let control_id = namespace.subscription_id("show-changes-control")?;
    broker.entities().create(&vehicle(&namespace, "show-changes")?).send().await?.done()?;
    for (id, endpoint, conformance) in [
        (&amended_id, "show-changes-amended", Some(ConformanceVersion::new(1, 3))),
        (&control_id, "show-changes-control", None),
    ] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_id.clone())])
            .maybe_ngsild_conformance(conformance)
            .notification(
                NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?)
                    .show_changes(ChangeReporting::WithPrevious)
                    .build()?,
            )
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    let fragment = EntityFragment::builder()
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Spire"), ("de", "Turmspitze")])?),
        )
        .build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let controls = sink.wait_for_at_least("show-changes-control", 1).await?;
    let amendments = sink.wait_for_at_least("show-changes-amended", 1).await?;
    let control = &controls.first().or_fail()?.body;
    // The conformance amendments write shapes of older versions the model reads as current ones, so the notifications
    // are read as received.
    expect_that!(control, json_equals("$.data[0].name.type", "LanguageProperty"));
    expect_that!(control, json_count("$.data[0].name.previousLanguageMap.*", 2));
    expect_that!(control, json_equals("$.data[0].name.previousLanguageMap.en", "Tower"));
    expect_that!(control, json_equals("$.data[0].name.previousLanguageMap.de", "Turm"));
    let amended = &amendments.first().or_fail()?.body;
    expect_that!(amended, json_equals("$.data[0].name.type", "Property"));
    expect_that!(amended, json_count("$.data[0].name.value.*", 2));
    expect_that!(amended, json_equals("$.data[0].name.value.en", "Spire"));
    expect_that!(amended, json_equals("$.data[0].name.value.de", "Turmspitze"));
    expect_that!(amended, json_count("$.data[0].name.previousValue.*", 2));
    expect_that!(amended, json_equals("$.data[0].name.previousValue.en", "Tower"));
    expect_that!(amended, json_equals("$.data[0].name.previousValue.de", "Turm"));
    expect_that!(amended, json_absent("$.data[0].name.previousLanguageMap"));
    expect_that!(amended, json_absent("$.data[0].name.languageMap"));
    broker.subscriptions().delete(&control_id).send().await?;
    broker.subscriptions().delete(&amended_id).send().await?;
    Ok(())
}
