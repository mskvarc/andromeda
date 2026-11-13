//! A q string holding an escaped control character reads back escaped and is accepted again.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Each control character a test writes into a String, with the escape IETF RFC 8259 section 7 writes it as: the
/// short form where the grammar has one, `\u` with lowercase hexadecimal digits otherwise.
const ESCAPED: [(char, &str); 6] = [
    ('\n', r"\n"),
    ('\t', r"\t"),
    ('\u{08}', r"\b"),
    ('\u{0C}', r"\f"),
    ('\r', r"\r"),
    ('\u{01}', r"\u0001"),
];

/// Pins: a Subscription created with q name=="a\nb", and with each of \t, \b, \f, \r and \u0001 in place of \n,
/// reads back with a q holding that escape, and a second Subscription created with the q read back answers 201
/// Clause: 4.9, 5.8.1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9008_a_q_string_holding_an_escaped_control_character_reads_back_escaped_and_is_accepted_again() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let building_type = namespace.entity_type("Building")?;
    for (index, (control, escape)) in ESCAPED.into_iter().enumerate() {
        let name = format!("a{control}b");
        let query = q::attr("name").eq(name.as_str()).build()?;
        let subscription_id = namespace.subscription_id(format!("escaped-{index}"))?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([building_type.as_str()])
            .q(query.clone())
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        let read_back = retrieved.q().or_fail()?.clone();
        expect_that!(read_back.to_string(), eq(&format!(r#"name=="a{escape}b""#)), "escape {escape}");
        expect_that!(&read_back, eq(&query), "escape {escape}");
        let again_id = namespace.subscription_id(format!("again-{index}"))?;
        let again = Subscription::builder()
            .id(again_id.clone())
            .entities([building_type.as_str()])
            .q(read_back)
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&again).send().await?;
        broker.subscriptions().delete(&again_id).send().await?;
        broker.subscriptions().delete(&subscription_id).send().await?;
    }
    Ok(())
}
