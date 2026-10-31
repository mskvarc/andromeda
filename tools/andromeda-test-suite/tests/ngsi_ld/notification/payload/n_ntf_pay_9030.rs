//! A subscription lang reduces a language property to that language.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
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

/// Pins: A Subscription with lang de is notified of an Entity created with name {en: Tower, de: Turm} as a Property
/// whose value is Turm and lang de
/// Clause: Table 5.2.12-1, 4.15, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9030_a_subscription_lang_reduces_a_language_property_to_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_lang_id = namespace.subscription_id("lang")?;
    let lang_tower_type = namespace.entity_type("LangTower")?;
    let lang_tower_id = namespace.urn("LangTower", 1)?;
    let subscription_lang = Subscription::builder()
        .id(subscription_lang_id.clone())
        .entities([lang_tower_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("lang")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (a subscription lang), so `lang` is written as text.
    let lang: RawJson = r#""de""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_lang.render())?.set_member("lang", &lang);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_lang,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let lang_tower = Entity::builder()
        .id(lang_tower_id.clone())
        .entity_type(lang_tower_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("de", "Turm")])?),
        )
        .build()?;
    broker.entities().create(&lang_tower).send().await?.done()?;
    sink.wait_for_at_least("lang", 1).await?;
    let notifications = sink.received("lang").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&lang_tower_id),
                has_attribute("name", property().value("Turm").lang("de").without(Member::LanguageMap))
            )
        )
    );
    broker.subscriptions().delete(&subscription_lang_id).send().await?;
    Ok(())
}
