//! A subscription q with an unquoted uri containing dots is stored.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Uri},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create Subscription accepts a q whose operand is an unquoted URI containing dots, for == and !=, and reads
/// it back unchanged
/// Clause: 4.9, 5.2.12, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_3060_a_subscription_q_with_an_unquoted_uri_containing_dots_is_stored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let thing_type = namespace.entity_type("Thing")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    // Table 5.2.12-1: q is "A valid query string as per clause 4.9"; CompEqualityValue admits a URI, which carries dots
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([thing_type.as_str()])
        .q(q::attr("link").eq(q::UnquotedUri::try_from("http://example.org/a".parse::<Uri>()?)?).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.q().map(ToString::to_string), some(eq("link==http://example.org/a")));
    let subscription_2_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([thing_type.as_str()])
        .q(q::attr("ref")
            .ne(q::UnquotedUri::try_from("https://sub.example.org/dir.v2/target.json".parse::<Uri>()?)?)
            .build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_2_2).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(
        subscription_3.q().map(ToString::to_string),
        some(eq("ref!=https://sub.example.org/dir.v2/target.json"))
    );
    let fragment = SubscriptionFragment::builder()
        .q(q::attr("ref")
            .eq(q::UnquotedUri::try_from("urn:example:release:1.5".parse::<Uri>()?)?)
            .build()?)
        .build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let subscription_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_4.q().map(ToString::to_string), some(eq("ref==urn:example:release:1.5")));
    Ok(())
}
