//! A registration created right after its subscription is reported once, however many such pairs race.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_registration::entity_registration,
    namespace::Namespace,
    notification_sink::NotificationSink,
    registration_report::{held_after_quiet_window, registration_subscription, report, reported_ids},
};
use futures::future::join_all;
use googletest::prelude::*;
use ngsi_ld::registration::{RegistrationFragment, TriggerReason};

/// The subscription and registration pairs created at once, so the broker evaluates some registration creations
/// while initial notifications are still pending.
const RACING_PAIRS: usize = 24;

/// Creates the `index`-th pair: a Context Source Registration Subscription selecting a type of its own, notifying
/// endpoint `index` of `sink`, then, the moment it is acknowledged, a registration of that type.
async fn create_pair(broker: &BrokerClient, namespace: &Namespace, sink: &NotificationSink, source: &ContextSource, index: usize) -> Result<()> {
    let entity_type = namespace.entity_type(&format!("Racing{index}"))?;
    let subscription = registration_subscription(namespace.subscription_id(index)?, &[&entity_type], sink, index)?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = entity_registration(
        &namespace.registration_id(index)?,
        &entity_type,
        namespace.id_pattern(&format!("Racing{index}"))?,
        source.endpoint(&index.to_string())?.as_str(),
    )?;
    broker.registrations().register(&registration).send().await?;
    Ok(())
}

/// Pins: 24 pairs created at once, each a Context Source Registration Subscription selecting a type of its own and,
/// the moment it is acknowledged, a registration of that type, notify each subscription first with one
/// `newlyMatching` notification carrying its registration alone; an update of each registration is then notified
/// to its subscription as `updated` with that registration alone, and no subscription receives a third notification
/// Clause: 5.11.2.4, 5.11.7, 5.3.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_5004_a_registration_created_right_after_its_subscription_is_reported_once_however_many_such_pairs_race() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registrations_cleanup = (0..RACING_PAIRS).try_fold(Cleanup::of(&broker), |cleanup, index| -> Result<Cleanup> {
        Ok(cleanup.registration(&namespace.registration_id(index)?))
    })?;
    join_all((0..RACING_PAIRS).map(|index| create_pair(&broker, &namespace, &sink, &source, index)))
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()?;
    for index in 0..RACING_PAIRS {
        let registration_id = namespace.registration_id(index)?;
        let first = report(&sink, index, 1).await?;
        expect_that!(first.subscription_id(), eq(&namespace.subscription_id(index)?));
        expect_that!(first.trigger_reason(), eq(TriggerReason::NewlyMatching));
        expect_that!(reported_ids(&first), elements_are![eq(&&registration_id)]);
    }
    for index in 0..RACING_PAIRS {
        let changed = RegistrationFragment::builder()
            .endpoint(source.endpoint(&format!("{index}-changed"))?)
            .build()?;
        broker.registrations().update(&namespace.registration_id(index)?, &changed).send().await?;
    }
    for index in 0..RACING_PAIRS {
        let registration_id = namespace.registration_id(index)?;
        let next = report(&sink, index, 2).await?;
        expect_that!(next.trigger_reason(), eq(TriggerReason::Updated));
        expect_that!(reported_ids(&next), elements_are![eq(&&registration_id)]);
    }
    expect_that!(held_after_quiet_window(&sink, 0..RACING_PAIRS).await?, each(eq(&2)));
    Ok(())
}
