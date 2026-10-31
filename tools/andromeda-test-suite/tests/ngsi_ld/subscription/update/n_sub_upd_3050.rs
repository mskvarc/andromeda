//! A boolean after an ordering operator in an updated q is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription refuses a q whose ordering operator is followed by true or false with `BadRequestData`
/// and keeps the stored q, and accepts the Boolean after ==
/// Clause: 4.9, 5.2.12, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_3050_a_boolean_after_an_ordering_operator_in_an_updated_q_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([sensor_type.as_str()])
        .q(q::attr("p").gt(1).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    // the stored q is a valid query string; an update replacing it with a Boolean after an ordering operator breaks the restriction of Table 5.2.12-1
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q: RawJson = r#""b>true""#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render())?.set_member("q", &q);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let fragment_2 = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q_2: RawJson = r#""b<=false""#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment_2.render())?.set_member("q", &q_2);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &fragment_2,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let subscription_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_2.q().map(ToString::to_string), some(eq("p>1")));
    // the Boolean after == is accepted
    let fragment_3 = SubscriptionFragment::builder().q(q::attr("b").eq(true).build()?).build()?;
    broker.subscriptions().update(&subscription_id, &fragment_3).send().await?;
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(subscription_3.q().map(ToString::to_string), some(eq("b==true")));
    Ok(())
}
