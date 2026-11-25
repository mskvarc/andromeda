//! A Subscription's q holding hops with type lists reads back as written.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription created with q a{T1,T2:b{c}}, and one created with q sensor{Device:humidity}==40, are each
/// retrieved with exactly that q
/// Clause: 4.9, 5.8.1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0019_a_subscription_q_holding_hops_with_type_lists_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let station_type = namespace.entity_type("WeatherStation")?;
    let queries = [
        ("a{T1,T2:b{c}}", q::attr("a").linked_typed(["T1", "T2"], "b").linked("c").exists().build()?),
        (
            "sensor{Device:humidity}==40",
            q::attr("sensor").linked_typed(["Device"], "humidity").eq(40).build()?,
        ),
    ];
    for (index, (written, query)) in queries.into_iter().enumerate() {
        expect_that!(query.to_string(), eq(written));
        let subscription_id = namespace.subscription_id(index)?;
        let subscription = Subscription::builder()
            .id(subscription_id.clone())
            .entities([station_type.as_str()])
            .q(query)
            .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .subscriptions()
            .retrieve(&subscription_id)
            .send()
            .await?;
        expect_that!(retrieved.q().map(ToString::to_string), some(eq(written)), "{written}");
        broker.subscriptions().delete(&subscription_id).send().await?;
    }
    Ok(())
}
