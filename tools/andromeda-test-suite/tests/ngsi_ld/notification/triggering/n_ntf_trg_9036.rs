//! A subscription q string holding an escaped control character notifies the entity holding that character.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The control characters a test writes into a String: line feed, tab, backspace, form feed, carriage return and
/// U+0001, which a q writes as `\n`, `\t`, `\b`, `\f`, `\r` and `\u0001`.
const CONTROLS: [char; 6] = ['\n', '\t', '\u{08}', '\u{0C}', '\r', '\u{01}'];

/// Pins: a Subscription with q name=="a\nb", and one each with \t, \b, \f, \r and \u0001 in place of \n, notifies the
/// Entity whose name holds a, that character and b, and not an Entity created before it whose name holds another
/// of them
/// Clause: 4.9, 5.8.1, 5.8.6
#[gtest]
#[tokio::test]
async fn n_ntf_trg_9036_a_subscription_q_string_holding_an_escaped_control_character_notifies_the_entity_holding_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let building_type = namespace.entity_type("Building")?;
    for (endpoint, control) in (1_usize..).zip(CONTROLS) {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(endpoint)?)
            .entities([building_type.as_str()])
            .q(q::attr("name").eq(format!("a{control}b").as_str()).build()?)
            .notification(
                NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?)
                    .format(Representation::Normalized)
                    .build()?,
            )
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    // The Entities are created in order, so an endpoint whose q also matched an earlier Entity's character would
    // hold that Entity's notification first.
    for (endpoint, control) in (1_usize..).zip(CONTROLS) {
        let building = Entity::builder()
            .id(namespace.urn("Building", endpoint)?)
            .entity_type(building_type.clone())
            .property("name", format!("a{control}b").as_str())
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    for endpoint in (1_usize..).take(CONTROLS.len()) {
        let notifications = sink.wait_for_at_least(endpoint, 1).await?;
        let notification = notifications.first().or_fail()?.entities(Lossless::Normalized)?;
        expect_that!(
            notification.data().as_slice(),
            nth(0, has_id(&namespace.urn("Building", endpoint)?)),
            "endpoint {endpoint}"
        );
    }
    for endpoint in (1_usize..).take(CONTROLS.len()) {
        broker.subscriptions().delete(&namespace.subscription_id(endpoint)?).send().await?;
    }
    Ok(())
}
