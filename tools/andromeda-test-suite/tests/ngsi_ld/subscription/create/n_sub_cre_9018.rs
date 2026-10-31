//! A json null member is bad request data naming it.

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

/// Pins: Create Subscription with null as `subscriptionName`, `jsonldContext`, `datasetId`, `expiresAt`, `isActive`
/// or `ngsildConformance` answers 400 `BadRequestData` naming the member and stores nothing
/// Clause: 5.2.1, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9018_a_json_null_member_is_bad_request_data_naming_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_json_null_subscriptionname_id = namespace.subscription_id("json-null-subscriptionname")?;
    let null_member_probe_type = namespace.entity_type("NullMemberProbe")?;
    let subscription_json_null_jsonldcontext_id = namespace.subscription_id("json-null-jsonldcontext")?;
    let subscription_json_null_datasetid_id = namespace.subscription_id("json-null-datasetid")?;
    let subscription_json_null_expiresat_id = namespace.subscription_id("json-null-expiresat")?;
    let subscription_json_null_isactive_id = namespace.subscription_id("json-null-isactive")?;
    let subscription_json_null_ngsildconformance_id = namespace.subscription_id("json-null-ngsildconformance")?;
    let subscription_json_null_subscriptionname = Subscription::builder()
        .id(subscription_json_null_subscriptionname_id.clone())
        .entities([null_member_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`subscriptionName` must be a string at subscriptionName), so `subscriptionName` is written as text.
    let subscription_name: RawJson = "null".parse()?;
    let document = UncheckedDocument::try_from(subscription_json_null_subscriptionname.render())?.set_member("subscriptionName", &subscription_name);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_json_null_subscriptionname,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("subscriptionName")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_json_null_subscriptionname_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_json_null_jsonldcontext = Subscription::builder()
        .id(subscription_json_null_jsonldcontext_id.clone())
        .entities([null_member_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`jsonldContext` must be a string at jsonldContext), so `jsonldContext` is written as text.
    let jsonld_context: RawJson = "null".parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_json_null_jsonldcontext.render())?.set_member("jsonldContext", &jsonld_context);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_json_null_jsonldcontext,
        })?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("jsonldContext")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_json_null_jsonldcontext_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let subscription_json_null_datasetid = Subscription::builder()
        .id(subscription_json_null_datasetid_id.clone())
        .entities([null_member_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (expected a JSON array at datasetId), so `datasetId` is written as text.
    let dataset_id: RawJson = "null".parse()?;
    let document_3 = UncheckedDocument::try_from(subscription_json_null_datasetid.render())?.set_member("datasetId", &dataset_id);
    let request_3 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_json_null_datasetid,
        })?
        .document(&document_3);
    let answer_5 = broker.send_unchecked(request_3).await;
    expect_that!(
        &answer_5,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("datasetId")
    );
    let answer_6 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_json_null_datasetid_id)
        .send()
        .await;
    expect_that!(&answer_6, answered(StatusCode::NOT_FOUND));
    let subscription_json_null_expiresat = Subscription::builder()
        .id(subscription_json_null_expiresat_id.clone())
        .entities([null_member_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`expiresAt` must be a string at expiresAt), so `expiresAt` is written as text.
    let expires_at: RawJson = "null".parse()?;
    let document_4 = UncheckedDocument::try_from(subscription_json_null_expiresat.render())?.set_member("expiresAt", &expires_at);
    let request_4 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_json_null_expiresat,
        })?
        .document(&document_4);
    let answer_7 = broker.send_unchecked(request_4).await;
    expect_that!(
        &answer_7,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("expiresAt")
    );
    let answer_8 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_json_null_expiresat_id)
        .send()
        .await;
    expect_that!(&answer_8, answered(StatusCode::NOT_FOUND));
    let subscription_json_null_isactive = Subscription::builder()
        .id(subscription_json_null_isactive_id.clone())
        .entities([null_member_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`isActive` must be a boolean at isActive), so `isActive` is written as text.
    let is_active: RawJson = "null".parse()?;
    let document_5 = UncheckedDocument::try_from(subscription_json_null_isactive.render())?.set_member("isActive", &is_active);
    let request_5 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_json_null_isactive,
        })?
        .document(&document_5);
    let answer_9 = broker.send_unchecked(request_5).await;
    expect_that!(
        &answer_9,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("isActive")
    );
    let answer_10 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_json_null_isactive_id)
        .send()
        .await;
    expect_that!(&answer_10, answered(StatusCode::NOT_FOUND));
    let subscription_json_null_ngsildconformance = Subscription::builder()
        .id(subscription_json_null_ngsildconformance_id.clone())
        .entities([null_member_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (`ngsildConformance` must be a string at ngsildConformance), so `ngsildConformance` is written as text.
    let ngsild_conformance: RawJson = "null".parse()?;
    let document_6 = UncheckedDocument::try_from(subscription_json_null_ngsildconformance.render())?.set_member("ngsildConformance", &ngsild_conformance);
    let request_6 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_json_null_ngsildconformance,
        })?
        .document(&document_6);
    let answer_11 = broker.send_unchecked(request_6).await;
    expect_that!(
        &answer_11,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("ngsildConformance")
    );
    let answer_12 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_json_null_ngsildconformance_id)
        .send()
        .await;
    expect_that!(&answer_12, answered(StatusCode::NOT_FOUND));
    Ok(())
}
