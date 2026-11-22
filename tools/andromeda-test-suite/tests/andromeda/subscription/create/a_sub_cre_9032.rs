//! A pattern a Subscription is created with in quotes reads back bare.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with q `brandName~="^Mer(c|z)edes$";speed>50` and Retrieve Subscription answers the q
/// with its pattern bare, `brandName~=^Mer(c|z)edes$;speed>50`, the one filter both spellings write
/// Clause: 4.9, 5.2.12, 6.10.3.1
/// Recorded: clause 5.2.12 asks for the q of a Subscription and not for its spelling; Andromeda writes every pattern
/// bare, as the clause 4.9 grammar writes a `RegExp`, unless the bare form would end it early or drop whitespace at
/// its ends
#[gtest]
#[tokio::test]
async fn a_sub_cre_9032_a_pattern_a_subscription_is_created_with_in_quotes_reads_back_bare() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id("quoted-pattern")?;
    let car_type = namespace.entity_type("Car")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([car_type.as_str()])
        .q(q::attr("brandName").matches("\"^Mer(c|z)edes$\"").and(q::attr("speed").gt(50)).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("quoted-pattern")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let bare = q::attr("brandName").matches("^Mer(c|z)edes$").and(q::attr("speed").gt(50)).build()?;
    expect_that!(retrieved.q(), some(eq(&bare)));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
