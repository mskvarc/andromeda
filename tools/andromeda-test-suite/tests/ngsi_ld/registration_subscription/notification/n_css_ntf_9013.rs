//! A slow endpoint delays no other periodic csource subscription.

use andromeda_test_suite::{broker, config::SuiteConfig, entity_registration::entity_registration, namespace::Namespace, notification_sink::NotificationSink};
use chrono::{TimeDelta, Utc};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::time::Duration;
use wiremock::ResponseTemplate;

/// How long the slow endpoint takes to answer each notification.
const SLOW_ANSWER: Duration = Duration::from_secs(8);

/// Pins: Context Source Registration Subscription A with `timeInterval` 1, whose endpoint answers 200 after 8 s, and
/// Subscription B with `timeInterval` 1, created right after A over the same registration with an endpoint answering
/// at once: B receives at least 3 notifications with a `notifiedAt` within 5 s of its creation, each before A's first
/// notification is answered
/// Clause: 5.11.7
/// Recorded: B is notified on creation and at its grid points 1 s to 5 s after it, six in all, so three leave three
/// of slack; A's first notification is answered no sooner than 8 s after its `notifiedAt`
#[gtest]
#[tokio::test]
async fn n_css_ntf_9013_a_slow_endpoint_delays_no_other_periodic_csource_subscription() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    sink.answer("slow", ResponseTemplate::new(200).set_delay(SLOW_ANSWER)).await?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let slow_id = namespace.subscription_id("slow")?;
    let prompt_id = namespace.subscription_id("prompt")?;
    let building_type = namespace.entity_type("Building")?;
    let registration = entity_registration(
        &registration_id,
        &building_type,
        namespace.id_pattern_ending(".+", ".*")?,
        config.unreachable_origin().as_str(),
    )?;
    broker.registrations().register(&registration).send().await?;
    let slow = Subscription::registration_builder()
        .id(slow_id.clone())
        .entities([building_type.as_str()])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("slow")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&slow).send().await?;
    let prompt = Subscription::registration_builder()
        .id(prompt_id.clone())
        .entities([building_type.as_str()])
        .time_interval(1)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint("prompt")?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let prompt_requested = Utc::now();
    broker.registration_subscriptions().create(&prompt).send().await?;
    let slow_first = sink.wait_for_at_least("slow", 1).await?;
    let slow_notified = chrono::DateTime::<Utc>::from(*slow_first.first().or_fail()?.registrations()?.notified_at());
    let slow_answered = slow_notified + TimeDelta::from_std(SLOW_ANSWER)?;
    let received = sink.wait_for_at_least("prompt", 3).await?;
    let mut notified_at = received
        .iter()
        .map(|notification| Ok(chrono::DateTime::<Utc>::from(*notification.registrations()?.notified_at())))
        .collect::<Result<Vec<_>>>()?;
    notified_at.sort_unstable();
    let notified_at = notified_at;
    let third = *notified_at.get(2).or_fail()?;
    expect_that!((third - prompt_requested).num_milliseconds(), le(5_000));
    expect_that!(third, lt(slow_answered));
    broker.registration_subscriptions().delete(&prompt_id).send().await?;
    broker.registration_subscriptions().delete(&slow_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
