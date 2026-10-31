//! A temporal q on an entity subscription is bad request data while a registration subscription keeps it.

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
    model::{DateTime, ErrorType, NgsiLdMediaType, RawJson},
    query::temporal_query::{TemporalQuery, TimeRelation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Create Subscription with a `temporalQ` answers 400 `BadRequestData` naming `temporalQ` and stores nothing,
/// and Create Context Source Registration Subscription with the same `temporalQ` is created
/// Clause: Table 5.2.12-1, 5.8.1.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_sub_cre_9025_a_temporal_q_on_an_entity_subscription_is_bad_request_data_while_a_registration_subscription_keeps_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_temporal_q_entity_id = namespace.subscription_id("temporal-q-entity")?;
    let temporal_probe_type = namespace.entity_type("TemporalProbe")?;
    let subscription_temporal_q_registration_id = namespace.subscription_id("temporal-q-registration")?;
    let subscription_temporal_q_entity = Subscription::builder()
        .id(subscription_temporal_q_entity_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (member `temporalQ` is not allowed here), so `temporalQ` is written as text.
    let temporal_q: RawJson = r#"{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(subscription_temporal_q_entity.render())?.set_member("temporalQ", &temporal_q);
    let request = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_temporal_q_entity,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("temporalQ")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_temporal_q_entity_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let subscription_temporal_q_registration = Subscription::registration_builder()
        .id(subscription_temporal_q_registration_id.clone())
        .entities([temporal_probe_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    // The model refuses it (a registration subscription temporalQ), so `temporalQ` is written as text.
    let temporal_q_2: RawJson = r#"{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(subscription_temporal_q_registration.render())?.set_member("temporalQ", &temporal_q_2);
    let request_2 = broker
        .unchecked(&CreateSubscription {
            subscription: &subscription_temporal_q_registration,
        })?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_3, answered(StatusCode::CREATED));
    let subscription = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&subscription_temporal_q_registration_id)
        .send()
        .await?;
    expect_that!(subscription.temporal_q().map(TemporalQuery::relation), some(eq(TimeRelation::After)));
    expect_that!(
        subscription.temporal_q().map(TemporalQuery::time_at).map(DateTime::timestamp),
        some(eq("2020-01-01T00:00:00Z".parse::<DateTime>()?.timestamp()))
    );
    broker
        .registration_subscriptions()
        .delete(&subscription_temporal_q_registration_id)
        .send()
        .await?;
    Ok(())
}
