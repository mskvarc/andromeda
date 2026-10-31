//! A notification is unaffected by the accept language of the subscription request.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
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
    http::headers::AcceptLanguage,
    model::{AttributeKind, Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};

/// Pins: a Subscription created with Accept-Language fr notifies the street `LanguageProperty` with its whole
/// `languageMap`, because the header of the creating request is no part of the Subscription
/// Clause: 5.8.1, 5.8.6
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0012_a_notification_is_unaffected_by_the_accept_language_of_the_subscription_request() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .notification_triggers([NotificationTrigger::EntityCreated])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker
        .with_header(AcceptLanguage::try_from("fr".parse::<LanguageFilter>()?)?)?
        .subscriptions()
        .create(&subscription)
        .send()
        .await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grand Place"), ("nl", "Grote Markt")])?),
        )
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(
        notification.data().as_slice(),
        nth(
            0,
            all!(
                has_id(&building_id),
                has_attribute(
                    "street",
                    attribute_of(AttributeKind::LanguageProperty)
                        .language("fr", "Grand Place")
                        .language("nl", "Grote Markt")
                        .without(Member::Lang)
                )
            )
        )
    );
    Ok(())
}
