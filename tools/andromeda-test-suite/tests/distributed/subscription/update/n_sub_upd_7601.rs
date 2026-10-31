//! A fragment is merged and a malformed one refused.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription on broker A with a description and with a q fragment answers 204 each and both read
/// back; a fragment whose entities is not an array answers 400 `BadRequestData`
/// Clause: 5.8.2.4, 5.5.8, 5.2.12
#[gtest]
#[tokio::test]
async fn n_sub_upd_7601_a_fragment_is_merged_and_a_malformed_one_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let fragment = SubscriptionFragment::builder().description("updated").build()?;
    broker.subscriptions().update(&subscription_id, &fragment).send().await?;
    let fragment_2 = SubscriptionFragment::builder().q(q::attr("speed").gt(10).build()?).build()?;
    broker.subscriptions().update(&subscription_id, &fragment_2).send().await?;
    let subscription_2 = broker.with_accept(Accept::any()).subscriptions().retrieve(&subscription_id).send().await?;
    expect_that!(subscription_2.description(), some(eq("updated")));
    expect_that!(subscription_2.q().map(ToString::to_string), some(eq("speed>10")));
    let fragment_3 = SubscriptionFragment::builder().build()?;
    // The model refuses it (expected a JSON array at entities), so `entities` is written as text.
    let entities: RawJson = r#""not-an-array""#.parse()?;
    let document = UncheckedDocument::try_from(fragment_3.render())?.set_member("entities", &entities);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment_3,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
