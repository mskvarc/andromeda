//! A temporal q naming no window is bad request data.

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

/// Pins: Create Context Source Registration Subscription with a `temporalQ` whose timerel before or after carries
/// an `endTimeAt`, or whose between closes before or at its `timeAt`, answers 400 `BadRequestData` naming
/// `endTimeAt` and stores nothing, and a between one microsecond wide is created
/// Clause: 4.11, Table 5.2.21-1, 5.5.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_cre_9008_a_temporal_q_naming_no_window_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_before_with_end_id = namespace.subscription_id("before-with-end")?;
    let temporal_probe_type = namespace.entity_type("TemporalProbe")?;
    let subscription_after_with_end_id = namespace.subscription_id("after-with-end")?;
    let subscription_inverted_id = namespace.subscription_id("inverted")?;
    let subscription_empty_id = namespace.subscription_id("empty")?;
    let subscription_well_formed_id = namespace.subscription_id("well-formed")?;
    let subscription_before_with_end = Subscription::registration_builder()
        .id(subscription_before_with_end_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ.endTimeAt), so `temporalQ` is written as text.
    let temporal_q: RawJson = r#"{"timerel": "before", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "2020-02-01T00:00:00Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_before_with_end.render())?.set_member("temporalQ", &temporal_q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_before_with_end,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_before_with_end_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_after_with_end = Subscription::registration_builder()
        .id(subscription_after_with_end_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid temporal query at temporalQ.endTimeAt), so `temporalQ` is written as text.
    let temporal_q_2: RawJson = r#"{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "2020-02-01T00:00:00Z"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_after_with_end.render())?.set_member("temporalQ", &temporal_q_2);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_after_with_end,
        })?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_after_with_end_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_inverted = Subscription::registration_builder()
        .id(subscription_inverted_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q_3: RawJson = r#"{"timerel": "between", "timeAt": "2020-02-01T00:00:00Z", "endTimeAt": "2020-01-01T00:00:00Z"}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(subscription_inverted.render())?.set_member("temporalQ", &temporal_q_3);
    let request_3 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_inverted,
        })?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_inverted_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let subscription_empty = Subscription::registration_builder()
        .id(subscription_empty_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q_4: RawJson = r#"{"timerel": "between", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "2020-01-01T00:00:00Z"}"#.parse()?;
    let document_4 = UncheckedDocument::try_from(subscription_empty.render())?.set_member("temporalQ", &temporal_q_4);
    let request_4 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_empty,
        })?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(
        &answer_7,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_empty_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let subscription_well_formed = Subscription::registration_builder()
        .id(subscription_well_formed_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q_5: RawJson = r#"{"timerel": "between", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "2020-01-01T00:00:00.000001Z"}"#.parse()?;
    let document_5 = UncheckedDocument::try_from(subscription_well_formed.render())?.set_member("temporalQ", &temporal_q_5);
    let request_5 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_well_formed,
        })?
        .document(&document_5);
    let answer_9 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_9, answered(StatusCode::CREATED));
    broker.registration_subscriptions().delete(&subscription_well_formed_id).send().await?;
    Ok(())
}
