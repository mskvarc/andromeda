//! An entity deletion notifies a deleted language property with a null language map.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType, Representation},
    subscription::{ChangeReporting, Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: Delete Entity notifies a Subscription with `entityDeleted` and `showChanges` of the Entity's
/// single-instance `LanguageProperty` with `languageMap` {"@none": "urn:ngsi-ld:null"} beside `previousLanguageMap`
/// holding the deleted map, in the normalized and in the concise format
/// Clause: 5.8.6, 4.5.18.2, 5.6.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9010_an_entity_deletion_notifies_a_deleted_language_property_with_a_null_language_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let street_type = namespace.entity_type("DeletedStreet")?;
    let street_id = namespace.urn("DeletedStreet", 1)?;
    let normalized_id = namespace.subscription_id("normalized")?;
    let concise_id = namespace.subscription_id("concise")?;
    let street = Entity::builder()
        .id(street_id.clone())
        .entity_type(street_type.clone())
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&street).send().await?.done()?;
    for (id, format, endpoint) in [
        (&normalized_id, Representation::Normalized, "normalized"),
        (&concise_id, Representation::Concise, "concise"),
    ] {
        let subscription = Subscription::builder()
            .id(id.clone())
            .entities([street_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityDeleted])
            .notification(
                NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?)
                    .format(format)
                    .show_changes(ChangeReporting::WithPrevious)
                    .build()?,
            )
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    broker.entities().delete(&street_id).send().await?.done()?;
    let normalized = sink.wait_for_at_least("normalized", 1).await?;
    expect_that!(normalized, len(eq(1)));
    // The spelling of the NGSI-LD Null in the deleted language map is the subject, so the notifications are read as
    // received.
    let body = &normalized.first().or_fail()?.body;
    expect_that!(body, json_equals("$.data[0].id", street_id.to_string()));
    expect_that!(body, json_equals("$.data[0].street.type", "LanguageProperty"));
    expect_that!(body, json_absent("$.data[0].street.languageMap.fr"));
    expect_that!(body, json_absent("$.data[0].street.languageMap.nl"));
    expect_that!(body, json_equals("$.data[0].street.languageMap['@none']", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].street.previousLanguageMap.fr", "Grand Place"));
    expect_that!(body, json_equals("$.data[0].street.previousLanguageMap.nl", "Grote Markt"));
    let concise = sink.wait_for_at_least("concise", 1).await?;
    expect_that!(concise, len(eq(1)));
    let body_2 = &concise.first().or_fail()?.body;
    expect_that!(body_2, json_equals("$.data[0].id", street_id.to_string()));
    expect_that!(body_2, json_absent("$.data[0].street.languageMap.fr"));
    expect_that!(body_2, json_absent("$.data[0].street.languageMap.nl"));
    expect_that!(body_2, json_equals("$.data[0].street.languageMap['@none']", "urn:ngsi-ld:null"));
    expect_that!(body_2, json_equals("$.data[0].street.previousLanguageMap.fr", "Grand Place"));
    expect_that!(body_2, json_equals("$.data[0].street.previousLanguageMap.nl", "Grote Markt"));
    broker.subscriptions().delete(&normalized_id).send().await?;
    broker.subscriptions().delete(&concise_id).send().await?;
    Ok(())
}
