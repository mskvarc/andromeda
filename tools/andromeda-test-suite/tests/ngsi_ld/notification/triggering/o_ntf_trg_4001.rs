//! A subscription q string ordering follows the unicode collation algorithm.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a subscription whose q is name>"c" notifies an entity named Delta, which sorts after the one-letter word c
/// under the Unicode Collation Algorithm, and not an entity named Bravo
/// Clause: 4.9, 5.8.6
/// Recorded: clause 4.9 says implementations "should support the Unicode Collation Algorithm (UCA)" for comparing text
/// strings, and 5.8.6 has a notification match the q of the subscription as clause 4.9 defines it; a "should", so code
/// point order also conforms. Under code point order Delta sorts before c and nothing is notified
#[gtest]
#[tokio::test]
async fn o_ntf_trg_4001_a_subscription_q_string_ordering_follows_the_unicode_collation_algorithm() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let word_type = namespace.entity_type("Word")?;
    let word_bravo_id = namespace.urn("Word", "bravo")?;
    let word_delta_id = namespace.urn("Word", "delta")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([word_type.as_str()])
        .q(q::attr("name").gt("c").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let word_bravo = Entity::builder()
        .id(word_bravo_id)
        .entity_type(word_type.clone())
        .property("name", "Bravo")
        .build()?;
    broker.entities().create(&word_bravo).send().await?.done()?;
    let word_delta = Entity::builder()
        .id(word_delta_id.clone())
        .entity_type(word_type.clone())
        .property("name", "Delta")
        .build()?;
    broker.entities().create(&word_delta).send().await?.done()?;
    sink.wait_for_at_least(1, 1).await?;
    // The oldest notification is the Delta entity: the entity created first does not match the q and sent none.
    let notifications = sink.received(1).await?;
    let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), nth(0, has_id(&word_delta_id)));
    Ok(())
}
