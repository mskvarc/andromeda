//! A subscription ranked lang falls to the second choice the entity holds.

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
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Representation},
    query::language_filter::LanguageFilter,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A Subscription with `lang` `ja,en;q=0.8,*;q=0.1` is notified of an Entity created with name {de: Turm, en:
/// Tower} as a Property whose value is Tower and lang en, the second choice, since the first choice ja is absent
/// Clause: Table 5.2.12-1, 4.15, 5.8.1
/// Recorded: clause 4.15 example 4 ranks the preferences, and the weight of the second choice outranks the wildcard
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9043_a_subscription_ranked_lang_falls_to_the_second_choice_the_entity_holds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("ranked-lang")?;
    let tower_type = namespace.entity_type("LangTower")?;
    let tower_id = namespace.urn("LangTower", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([tower_type.as_str()])
        .lang("ja,en;q=0.8,*;q=0.1".parse::<LanguageFilter>()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("ranked")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let tower = Entity::builder()
        .id(tower_id.clone())
        .entity_type(tower_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "Turm"), ("en", "Tower")])?),
        )
        .build()?;
    broker.entities().create(&tower).send().await?.done()?;
    sink.wait_for_at_least("ranked", 1).await?;
    let notifications = sink.received("ranked").await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&tower_id),
                has_attribute("name", property().value("Tower").lang("en").without(Member::LanguageMap))
            )
        )
    );
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
