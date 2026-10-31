//! A temporal q in an entity subscription update is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Update Subscription with a fragment carrying a well-formed `temporalQ` answers 400 `BadRequestData` naming
/// `temporalQ`, and the Subscription reads back without a `temporalQ`
/// Clause: Table 5.2.12-1, 5.8.2.4
#[gtest]
#[tokio::test]
async fn n_sub_upd_9014_a_temporal_q_in_an_entity_subscription_update_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id(1)?;
    let temporal_probe_type = namespace.entity_type("TemporalProbe")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    // The model writes no `temporalQ` in an entity subscription fragment, so the fragment is sent as written.
    let request = json_request_to(
        &broker,
        Method::PATCH,
        &format!("subscriptions/{subscription_id}"),
        r#"{"temporalQ": {"timerel": "between", "timeAt": "2020-01-01T00:00:00Z", "endTimeAt": "2020-02-01T00:00:00Z"}}"#,
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_body_containing("temporalQ")
    );
    // The reader refuses `temporalQ` in an entity subscription (Table 5.2.12-1), so the read pins its absence.
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    expect_that!(stored.id(), eq(&subscription_id));
    broker.subscriptions().delete(&subscription_id).send().await?;
    Ok(())
}
