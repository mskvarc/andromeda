//! An ngsild conformance that is not major dot minor is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateSubscription, UpdateSubscription},
    },
    model::{ErrorType, NgsiLdMediaType, RawJson},
    query::{EntitySelector, builders::SelectorTypeBuilder},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Create Subscription with `ngsildConformance` "1", "1.2.3", "abc", "", "v1.4", the number 1.4 or null
/// answers 400 `BadRequestData` and stores nothing, and Update Subscription with "1.2.3" answers 400
/// `BadRequestData`
/// Clause: Table 5.2.12-1, 5.5.4, 5.8.1.4, 5.8.3.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9016_an_ngsild_conformance_that_is_not_major_dot_minor_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_malformed_0_id = namespace.subscription_id("malformed-0")?;
    let vehicle_malformed_id = namespace.urn("Vehicle", "malformed")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_malformed_1_id = namespace.subscription_id("malformed-1")?;
    let subscription_malformed_2_id = namespace.subscription_id("malformed-2")?;
    let subscription_malformed_3_id = namespace.subscription_id("malformed-3")?;
    let subscription_malformed_4_id = namespace.subscription_id("malformed-4")?;
    let subscription_malformed_5_id = namespace.subscription_id("malformed-5")?;
    let subscription_malformed_6_id = namespace.subscription_id("malformed-6")?;
    let subscription_malformed_update_id = namespace.subscription_id("malformed-update")?;
    let subscription_malformed_0 = Subscription::builder()
        .id(subscription_malformed_0_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid conformance version at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance: RawJson = r#""1""#.parse()?;
    let document = UncheckedDocument::try_from(subscription_malformed_0.render())?.set_member("ngsildConformance", &ngsild_conformance);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_malformed_0,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_malformed_0_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_malformed_1 = Subscription::builder()
        .id(subscription_malformed_1_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid conformance version at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_2: RawJson = r#""1.2.3""#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_malformed_1.render())?.set_member("ngsildConformance", &ngsild_conformance_2);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_malformed_1,
        })?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_malformed_1_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_malformed_2 = Subscription::builder()
        .id(subscription_malformed_2_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid conformance version at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_3: RawJson = r#""abc""#.parse()?;
    let document_3 = UncheckedDocument::try_from(subscription_malformed_2.render())?.set_member("ngsildConformance", &ngsild_conformance_3);
    let request_3 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_malformed_2,
        })?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_malformed_2_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let subscription_malformed_3 = Subscription::builder()
        .id(subscription_malformed_3_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid conformance version at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_4: RawJson = r#""""#.parse()?;
    let document_4 = UncheckedDocument::try_from(subscription_malformed_3.render())?.set_member("ngsildConformance", &ngsild_conformance_4);
    let request_4 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_malformed_3,
        })?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_7, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_malformed_3_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let subscription_malformed_4 = Subscription::builder()
        .id(subscription_malformed_4_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (invalid conformance version at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_5: RawJson = r#""v1.4""#.parse()?;
    let document_5 = UncheckedDocument::try_from(subscription_malformed_4.render())?.set_member("ngsildConformance", &ngsild_conformance_5);
    let request_5 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_malformed_4,
        })?
        .document(&document_5);
    let answer_9 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_9, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_malformed_4_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    let subscription_malformed_5 = Subscription::builder()
        .id(subscription_malformed_5_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`ngsildConformance` must be a string at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_6: RawJson = "1.4".parse()?;
    let document_6 = UncheckedDocument::try_from(subscription_malformed_5.render())?.set_member("ngsildConformance", &ngsild_conformance_6);
    let request_6 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_malformed_5,
        })?
        .document(&document_6);
    let answer_11 = broker.send_unchecked(request_6).await;
    expect_that!(&answer_11, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_malformed_5_id)
        .send()
        .await;
    expect_that!(&answer_12, answered(StatusCode::NOT_FOUND));
    let subscription_malformed_6 = Subscription::builder()
        .id(subscription_malformed_6_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`ngsildConformance` must be a string at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_7: RawJson = "null".parse()?;
    let document_7 = UncheckedDocument::try_from(subscription_malformed_6.render())?.set_member("ngsildConformance", &ngsild_conformance_7);
    let request_7 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_malformed_6,
        })?
        .document(&document_7);
    let answer_13 = broker.send_unchecked(request_7).await;
    expect_that!(&answer_13, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_14 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_malformed_6_id)
        .send()
        .await;
    expect_that!(&answer_14, answered(StatusCode::NOT_FOUND));
    let subscription_malformed_update = Subscription::builder()
        .id(subscription_malformed_update_id.clone())
        .entities([EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_ids(vehicle_malformed_id.clone())])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription_malformed_update).send().await?;
    let fragment = SubscriptionFragment::builder().build()?;
    // The model refuses it (invalid conformance version at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance_8: RawJson = r#""1.2.3""#.parse()?;
    let document_8 = UncheckedDocument::try_from(fragment.render())?.set_member("ngsildConformance", &ngsild_conformance_8);
    let request_8 = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_malformed_update_id,
            fragment: &fragment,
        })?
        .document(&document_8);
    let answer_15 = broker.send_unchecked(request_8).await;
    expect_that!(&answer_15, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker.subscriptions().delete(&subscription_malformed_update_id).send().await?;
    Ok(())
}
