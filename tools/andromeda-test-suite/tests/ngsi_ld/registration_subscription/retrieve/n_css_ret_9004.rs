//! A csource subscription q string holding an escaped control character reads back escaped.

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

/// Pins: a Context Source Registration Subscription created with q name=="a\nb", and with each of \t, \b, \f, \r and
/// \u0001 in place of \n, reads back with a q holding that escape
/// Clause: 4.9, 5.11.2, 5.11.4
#[gtest]
#[tokio::test]
async fn n_css_ret_9004_a_csource_subscription_q_string_holding_an_escaped_control_character_reads_back_escaped() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for (index, (control, escape)) in ESCAPED.into_iter().enumerate() {
        let query = q::attr("name").eq(format!("a{control}b").as_str()).build()?;
        let subscription_id = namespace.subscription_id(index)?;
        let subscription = Subscription::registration_builder()
            .id(subscription_id.clone())
            .entities([vehicle_type.as_str()])
            .q(query.clone())
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.registration_subscriptions().create(&subscription).send().await?;
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registration_subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        expect_that!(
            retrieved.q().map(ToString::to_string),
            some(eq(&format!(r#"name=="a{escape}b""#))),
            "escape {escape}"
        );
        expect_that!(retrieved.q(), some(eq(&query)), "escape {escape}");
        broker.registration_subscriptions().delete(&subscription_id).send().await?;
    }
    Ok(())
}
