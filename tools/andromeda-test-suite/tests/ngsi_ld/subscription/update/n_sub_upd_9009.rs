//! A temporal q in an entity subscription fragment is bad request data and changes nothing.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::UpdateSubscription},
    model::{ErrorType, Input, NgsiLdMediaType, RawJson},
    query::temporal_query::TemporalQuery,
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionFragment},
};

/// Pins: Update Subscription with a `temporalQ` answers 400 `BadRequestData` naming `temporalQ` and changes
/// nothing, Update Context Source Registration Subscription with the same `temporalQ` answers 204, and an NGSI-LD
/// Null `temporalQ` in a Subscription fragment is accepted
/// Clause: Table 5.2.12-1, 5.5.8, 5.8.2.4, 5.11.3
#[gtest]
#[tokio::test]
async fn n_sub_upd_9009_a_temporal_q_in_an_entity_subscription_fragment_is_bad_request_data_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let subscription_id = namespace.subscription_id("temporal-q-update")?;
    let registration_subscription_id = namespace.subscription_id("temporal-q-registration-update")?;
    let probe_type = namespace.entity_type("TemporalProbe")?;
    let notification = || -> Result<NotificationParams<Input>> {
        Ok(NotificationParams::builder(Endpoint::builder(format!("{unreachable}/notify")).accept(NgsiLdMediaType::Json).build()?).build()?)
    };
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([probe_type.as_str()])
        .notification(notification()?)
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let empty = SubscriptionFragment::builder().build()?;
    // An entity subscription fragment has no temporalQ, so that member is set on the rendered fragment.
    let temporal_q: RawJson = r#"{"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(empty.render())?.set_member("temporalQ", &temporal_q);
    let request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &empty,
        })?
        .document(&document);
    let refused = broker.send_unchecked(request).await;
    expect_that!(
        &refused,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("temporalQ")
    );
    // The entity subscription reader refuses a `temporalQ` member, so a read that succeeds proves it absent.
    broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await?;
    let nulled_document = UncheckedDocument::try_from(empty.render())?.null_member("temporalQ");
    let nulled_request = broker
        .unchecked(&UpdateSubscription {
            id: &subscription_id,
            fragment: &empty,
        })?
        .document(&nulled_document);
    let nulled = broker.send_unchecked(nulled_request).await;
    expect_that!(&nulled, answered(StatusCode::NO_CONTENT));
    broker.subscriptions().delete(&subscription_id).send().await?;
    let after = TemporalQuery::after("2020-01-01T00:00:00Z".parse()?);
    let registration_subscription = Subscription::registration_builder()
        .id(registration_subscription_id.clone())
        .entities([probe_type.as_str()])
        .notification(notification()?)
        .build()?;
    broker.registration_subscriptions().create(&registration_subscription).send().await?;
    let registration_fragment = SubscriptionFragment::registration_builder().temporal_q(after.clone()).build()?;
    broker
        .registration_subscriptions()
        .update(&registration_subscription_id, &registration_fragment)
        .send()
        .await?;
    let registration_stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registration_subscriptions()
        .retrieve(&registration_subscription_id)
        .send()
        .await?;
    expect_that!(registration_stored.temporal_q(), some(eq(&after)));
    broker.registration_subscriptions().delete(&registration_subscription_id).send().await?;
    Ok(())
}
