//! A malformed q is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateSubscription},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with q "(`invalidQuery`" answers 400 `BadRequestData` with a title and stores nothing
/// Clause: 4.9, 5.5.4, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9015_a_malformed_q_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_bad_q_id = namespace.subscription_id("bad-q")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_bad_q = Subscription::builder()
        .id(subscription_bad_q_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid query language text at q), so `q` is written as text.
    let q: RawJson = r#""(invalidQuery""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_bad_q.render())?.set_member("q", &q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_bad_q,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_title());
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_bad_q_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
