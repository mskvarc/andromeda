//! A subscription q pattern with a language reads only that language.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose q is street`[nl]`~="Grote.*" notifies an entity whose nl string matches the pattern
/// and not an entity that holds only a fr entry
/// Clause: 4.9, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_4002_a_subscription_q_pattern_with_a_language_reads_only_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let place_type = namespace.entity_type("Place")?;
    let place_french_id = namespace.urn("Place", "french")?;
    let place_market_id = namespace.urn("Place", "market")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([place_type.as_str()])
        .q(q::attr("street").member("nl").matches("\"Grote.*\"").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let place_french = Entity::builder()
        .id(place_french_id)
        .entity_type(place_type.clone())
        .language_property("street", LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Grote Place")])?))
        .build()?;
    broker.entities().create(&place_french).send().await?.done()?;
    let place_market = Entity::builder()
        .id(place_market_id.clone())
        .entity_type(place_type.clone())
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("nl", "Grote Markt"), ("fr", "Grand Place")])?),
        )
        .build()?;
    broker.entities().create(&place_market).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    // The oldest notification is the nl entity: the entity created first holds no nl string and sent none.
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&place_market_id)));
    Ok(())
}
