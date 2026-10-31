//! An update selecting registrations created before it is notified once with both, however many such selections race.

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
use ngsi_ld::{
    model::EntityTypeName,
    registration::{RegistrationId, TriggerReason},
    subscription::SubscriptionFragment,
};

/// The selections run at once, so the broker reaches some registration creations, and some subscription creations,
/// only after the update that follows them has committed.
const RACING_SELECTIONS: usize = 24;

/// The two kinds of registration each selection creates, each the base of an entity type and of an id pattern.
const SELECTED_KINDS: [&str; 2] = ["Vehicle", "Bus"];

/// The id of the registration of `kind` the `index`-th selection creates.
fn selected_registration_id(namespace: &Namespace, index: usize, kind: &str) -> Result<RegistrationId> {
    Ok(namespace.registration_id(format!("{index}-{kind}"))?)
}

/// Runs the `index`-th selection: a Context Source Registration Subscription notifying endpoint `index` of `sink`
/// selecting a type no registration has, two registrations of two other types, and the update that makes the
/// subscription select exactly those two types the moment the second registration is acknowledged.
async fn run_selection(broker: &BrokerClient, namespace: &Namespace, sink: &NotificationSink, source: &ContextSource, index: usize) -> Result<()> {
    let unmatched_type = namespace.entity_type(&format!("Unmatched{index}"))?;
    let subscription_id = namespace.subscription_id(index)?;
    let subscription = registration_subscription(namespace.subscription_id(index)?, &[&unmatched_type], sink, index)?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let selected_types = SELECTED_KINDS
        .iter()
        .map(|kind| Ok(namespace.entity_type(&format!("{kind}{index}"))?))
        .collect::<Result<Vec<_>>>()?;
    for (kind, entity_type) in SELECTED_KINDS.iter().zip(&selected_types) {
        let registration = entity_registration(
            &selected_registration_id(namespace, index, kind)?,
            entity_type,
            namespace.id_pattern(&format!("{kind}{index}"))?,
            source.endpoint(&format!("{index}-{kind}"))?.as_str(),
        )?;
        broker.registrations().register(&registration).send().await?;
    }
    let widened = SubscriptionFragment::registration_builder()
        .entities(selected_types.iter().map(EntityTypeName::as_str))
        .build()?;
    broker.registration_subscriptions().update(&subscription_id, &widened).send().await?;
    Ok(())
}

/// Pins: 24 selections run at once, each a Context Source Registration Subscription selecting a type no registration
/// has, two registrations of two other types created while it selects neither, and the update that makes it select
/// both types, notify each subscription with one `newlyMatching` notification carrying both registrations and with
/// no other notification
/// Clause: 5.11.2.4, 5.11.3.4, 5.11.7, 5.3.3
#[gtest]
#[tokio::test]
async fn n_css_ntf_5005_an_update_selecting_registrations_created_before_it_is_notified_once_with_both_however_many_such_selections_race() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registrations_cleanup = (0..RACING_SELECTIONS)
        .flat_map(|index| SELECTED_KINDS.map(|kind| (index, kind)))
        .try_fold(Cleanup::of(&broker), |cleanup, (index, kind)| -> Result<Cleanup> {
            Ok(cleanup.registration(&selected_registration_id(&namespace, index, kind)?))
        })?;
    join_all((0..RACING_SELECTIONS).map(|index| run_selection(&broker, &namespace, &sink, &source, index)))
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()?;
    for index in 0..RACING_SELECTIONS {
        let vehicle_registration_id = selected_registration_id(&namespace, index, "Vehicle")?;
        let bus_registration_id = selected_registration_id(&namespace, index, "Bus")?;
        let notification = report(&sink, index, 1).await?;
        expect_that!(notification.subscription_id(), eq(&namespace.subscription_id(index)?));
        expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
        expect_that!(
            reported_ids(&notification),
            unordered_elements_are![eq(&&vehicle_registration_id), eq(&&bus_registration_id)]
        );
    }
    expect_that!(held_after_quiet_window(&sink, 0..RACING_SELECTIONS).await?, each(eq(&1)));
    Ok(())
}
