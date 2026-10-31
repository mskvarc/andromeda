//! A temporal q breaking its data type restriction is bad request data and stores nothing.

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

/// Pins: Create Context Source Registration Subscription with a `temporalQ` missing `timeAt`, missing timerel, with
/// timerel during, a `timeAt` or `endTimeAt` that is no `DateTime`, timeproperty `expiresAt` or `lastN` 0 answers
/// 400 `BadRequestData` and stores nothing
/// Clause: Table 5.2.21-1, 5.2.12, 5.5.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_cre_9007_a_temporal_q_breaking_its_data_type_restriction_is_bad_request_data_and_stores_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_no_time_at_id = namespace.subscription_id("no-time-at")?;
    let building_type = namespace.entity_type("Building")?;
    let subscription_no_timerel_id = namespace.subscription_id("no-timerel")?;
    let subscription_during_id = namespace.subscription_id("during")?;
    let subscription_time_at_word_id = namespace.subscription_id("time-at-word")?;
    let subscription_end_time_at_word_id = namespace.subscription_id("end-time-at-word")?;
    let subscription_expires_at_id = namespace.subscription_id("expires-at")?;
    let subscription_zero_last_n_id = namespace.subscription_id("zero-last-n")?;
    let subscription_no_time_at = Subscription::registration_builder()
        .id(subscription_no_time_at_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ), so `temporalQ` is written as text.
    let temporal_q: RawJson = r#"{"timerel": "after"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_no_time_at.render())?.set_member("temporalQ", &temporal_q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_no_time_at,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_no_time_at_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_no_timerel = Subscription::registration_builder()
        .id(subscription_no_timerel_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ), so `temporalQ` is written as text.
    let temporal_q_2: RawJson = r#"{"timeAt": "2020-01-01T00:00:00Z"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_no_timerel.render())?.set_member("temporalQ", &temporal_q_2);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_no_timerel,
        })?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_no_timerel_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_during = Subscription::registration_builder()
        .id(subscription_during_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ.timerel), so `temporalQ` is written as text.
    let temporal_q_3: RawJson = r#"{"timerel": "during", "timeAt": "2020-01-01T00:00:00Z"}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(subscription_during.render())?.set_member("temporalQ", &temporal_q_3);
    let request_3 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_during,
        })?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_during_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let subscription_time_at_word = Subscription::registration_builder()
        .id(subscription_time_at_word_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ.timeAt), so `temporalQ` is written as text.
    let temporal_q_4: RawJson = r#"{"timerel": "after", "timeAt": "yesterday"}"#.parse()?;
    let document_4 = UncheckedDocument::try_from(subscription_time_at_word.render())?.set_member("temporalQ", &temporal_q_4);
    let request_4 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_time_at_word,
        })?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_time_at_word_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let subscription_end_time_at_word = Subscription::registration_builder()
        .id(subscription_end_time_at_word_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ.endTimeAt), so `temporalQ` is written as text.
    let temporal_q_5: RawJson = r#"{"timerel": "between", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "tomorrow"}"#.parse()?;
    let document_5 = UncheckedDocument::try_from(subscription_end_time_at_word.render())?.set_member("temporalQ", &temporal_q_5);
    let request_5 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_end_time_at_word,
        })?
        .document(&document_5);
    let answer_9 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_end_time_at_word_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    let subscription_expires_at = Subscription::registration_builder()
        .id(subscription_expires_at_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ.timeproperty), so `temporalQ` is written as text.
    let temporal_q_6: RawJson = r#"{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z", "timeproperty": "expiresAt"}"#.parse()?;
    let document_6 = UncheckedDocument::try_from(subscription_expires_at.render())?.set_member("temporalQ", &temporal_q_6);
    let request_6 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_expires_at,
        })?
        .document(&document_6);
    let answer_11 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_expires_at_id)
        .send()
        .await;
    expect_that!(&answer_12, answered(StatusCode::NOT_FOUND));
    let subscription_zero_last_n = Subscription::registration_builder()
        .id(subscription_zero_last_n_id.clone())
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ.lastN), so `temporalQ` is written as text.
    let temporal_q_7: RawJson = r#"{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z", "lastN": 0}"#.parse()?;
    let document_7 = UncheckedDocument::try_from(subscription_zero_last_n.render())?.set_member("temporalQ", &temporal_q_7);
    let request_7 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_zero_last_n,
        })?
        .document(&document_7);
    let answer_13 = broker.send_unchecked(request_7).await;
    expect_that!(&answer_13, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_14 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_zero_last_n_id)
        .send()
        .await;
    expect_that!(&answer_14, answered(StatusCode::NOT_FOUND));
    Ok(())
}
