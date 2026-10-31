//! A bare word q value is bad request data and stores nothing.

use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::{ErrorType, NgsiLdMediaType, RawJson, Representation},
    query::q,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create Subscription and Update Subscription with a q holding an unquoted bare word are refused with
/// `BadRequestData`, nothing is stored and the stored q is kept, and the quoted string is accepted
/// Clause: 4.9, 5.8.1.4, 5.8.2, 5.2.12, 6.3.2
#[gtest]
#[tokio::test]
async fn n_sub_cre_4002_a_bare_word_q_value_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subscription_id = namespace.subscription_id(1)?;
    let device_type = namespace.entity_type("Device")?;
    let subscription_2_id = namespace.subscription_id(2)?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([device_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q: RawJson = r#""status==active""#.parse()?;
    let document = UncheckedDocument::try_from(subscription.render())?.set_member("q", &q);
    let request = broker.unchecked(&CreateSubscription { subscription: &subscription })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let subscription_2 = Subscription::builder()
        .id(subscription_2_id.clone())
        .entities([device_type.as_str()])
        .q(q::attr("status").eq("active").build()?)
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(2)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription_2).send().await?;
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q_2: RawJson = r#""status!=idle""#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render())?.set_member("q", &q_2);
    let request_2 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_2_id,
            fragment: &fragment,
        })?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_2_id)
        .send()
        .await?;
    expect_that!(subscription_3.q().map(ToString::to_string), some(eq("status==\"active\"")));
    Ok(())
}
