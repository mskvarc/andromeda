//! A subscription lang with no matching entry notifies the none string.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    nth::nth,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateSubscription},
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, RawJson, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a notification of a subscription whose lang is de carries a `LanguageProperty` that holds no de entry as a
/// Property with the @none string and the lang @none
/// Clause: 5.2.12, 5.8.6, 5.7.2.5, 4.15
/// Recorded: Table 5.2.12-1 says the subscription lang is the "Language filter to be applied to the query (clause
/// 4.15)", which 5.7.2.5 completes with the @none fallback
#[gtest]
#[tokio::test]
async fn n_ntf_pay_4001_a_subscription_lang_with_no_matching_entry_notifies_the_none_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription lang), so `lang` is written as text.
    let lang: RawJson = r#""de""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("lang", &lang);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "red"), ("@none", "x")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(0, all!(has_id(&building_id), has_attribute("label", property().value("x").lang("@none"))))
    );
    Ok(())
}
