//! A q comparing with a URI operand reads back as written.

use andromeda_test_suite::{body_json::json_equals, broker, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::q::Query,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription created with q r=="HTTP://Example.com/a", an operand whose RFC 3986 normalised spelling
/// differs from the written one, reads back with `q` exactly as written; the Subscription holds the query the client
/// sent, whatever spelling the broker compares with
/// Clause: 4.9, Table 5.2.12-1, 5.8.3
#[gtest]
#[tokio::test]
async fn n_sub_ret_9009_a_q_comparing_with_a_uri_operand_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("uri-operand")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .q(r#"r=="HTTP://Example.com/a""#.parse::<Query>()?)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let stored = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(&stored.body, json_equals("$.q", r#"r=="HTTP://Example.com/a""#));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
