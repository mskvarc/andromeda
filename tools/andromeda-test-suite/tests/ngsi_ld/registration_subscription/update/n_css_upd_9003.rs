//! A temporal q breaking its data type restriction is bad request data and keeps the stored one.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::temporal_query::{self, TemporalQuery},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Context Source Registration Subscription with a `temporalQ` missing `timeAt`, a between `temporalQ`
/// without `endTimeAt` or a `temporalQ` with `lastN` 0 answers 400 `BadRequestData` each time and the stored
/// `temporalQ` reads back as created
/// Clause: Table 5.2.21-1, 5.2.12, 5.5.4, 5.11.3.4
#[gtest]
#[tokio::test]
async fn n_css_upd_9003_a_temporal_q_breaking_its_data_type_restriction_is_bad_request_data_and_keeps_the_stored_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([building_type.as_str()])
        .temporal_q(temporal_query::after("2020-01-01T00:00:00Z".parse()?).build()?)
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let path = format!("csourceSubscriptions/{subscription_id}");
    // Each fragment breaks the temporalQ data type, which the typed query refuses, so it is sent as written.
    let request = json_request_to(&broker, Method::PATCH, &path, r#"{"temporalQ": {"timerel": "before"}}"#)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let request_2 = json_request_to(
        &broker,
        Method::PATCH,
        &path,
        r#"{"temporalQ": {"timerel": "between", "timeAt": "2021-01-01T00:00:00Z"}}"#,
    )?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("endTimeAt")
    );
    let request_3 = json_request_to(
        &broker,
        Method::PATCH,
        &path,
        r#"{"temporalQ": {"timerel": "before", "timeAt": "2021-01-01T00:00:00Z", "lastN": 0}}"#,
    )?;
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(retrieved.temporal_q(), some(eq(&TemporalQuery::after("2020-01-01T00:00:00Z".parse()?))));
    broker.registration_subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
