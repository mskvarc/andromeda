//! A subscription with a full tag notifies on that tag alone.

use andromeda_test_suite::{body_pattern::BodyPattern, broker, config::BrokerRole, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, LanguageMap, LanguageProperty, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: at the language-tags level, a Subscription with q label`[en-GB]`=="Lift" and the `entityCreated` trigger
/// is created, notifies on an entity whose `languageMap` holds the operand under en-GB and does not notify on one
/// holding it under en
/// Clause: 4.9, 5.8.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: the language-tags level of the `q_target_languages` extension admits
/// every RFC 5646 tag as a q target language, which the clause 4.9 `AttrName` cannot spell, and keeps the clause
/// 4.9 tag equality
#[gtest]
#[tokio::test]
async fn a_ext_ltg_0005_a_subscription_with_a_full_tag_notifies_on_that_tag_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let sink = NotificationSink::start().await?;
    let broker_language_tags = broker::client(BrokerRole::LanguageTags)?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_id = namespace.urn("Sensor", 1)?;
    let sensor_2_id = namespace.urn("Sensor", 2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .q(q::attr("label").member("en-GB").eq("Lift").build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker_language_tags.subscriptions().create(&subscription).send().await?;
    let sensor = Entity::builder()
        .id(sensor_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Lift")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor).send().await?.done()?;
    let sensor_2 = Entity::builder()
        .id(sensor_2_id)
        .entity_type(sensor_type.clone())
        .language_property("label", LanguageProperty::builder(LanguageMap::try_from_pairs([("en-GB", "Lift")])?))
        .build()?;
    broker_language_tags.entities().create(&sensor_2).send().await?.done()?;
    let selected = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Sensor:{namespace}-2')]").parse()?,
    )];
    sink.wait_for_at_least_where(1, &selected, 1).await?;
    let selected_2 = [BodyPattern::SelectsBelow(
        "$.data".parse()?,
        format!("$[?(@.id == 'urn:ngsi-ld:Sensor:{namespace}-1')]").parse()?,
    )];
    let notifications = sink.received_where(1, &selected_2).await?;
    expect_that!(notifications, len(eq(0)));
    Ok(())
}
