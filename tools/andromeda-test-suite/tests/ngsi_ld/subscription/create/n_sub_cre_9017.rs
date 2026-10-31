//! A null uri jsonld context is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with `jsonldContext` "urn:ngsi-ld:null" answers 400 `BadRequestData` naming
/// `jsonldContext` and stores nothing
/// Clause: 4.5.0, 5.2.1, 5.8.1.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9017_a_null_uri_jsonld_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_null_context_id = namespace.subscription_id("null-context")?;
    let null_context_probe_type = namespace.entity_type("NullContextProbe")?;
    let subscription_null_context = Subscription::builder()
        .id(subscription_null_context_id.clone())
        .entities([null_context_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .jsonld_context("urn:ngsi-ld:null")
        .build()?;
    let answer = broker.with_responses().subscriptions().create(&subscription_null_context).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("jsonldContext")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_null_context_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
