//! A `datasetId` on a csource subscription notifies the registrations that list it or list none.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{DatasetId, NgsiLdMediaType, Output},
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo, RegistrationMode, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// The identifiers of the registrations the notifications of sink number `number` carry, all of them `newlyMatching`.
async fn notified_registrations(sink: &NotificationSink, number: u32) -> Result<Vec<RegistrationId>> {
    let mut identifiers = Vec::new();
    for received in sink.received(number).await? {
        let notification = received.registrations()?;
        verify_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching))?;
        identifiers.extend(
            notification
                .data()
                .iter()
                .map(|registration| CSourceRegistration::<Output>::id(registration).clone()),
        );
    }
    Ok(identifiers)
}

/// Pins: Over registrations of the type listing dataset a, listing dataset b and listing none, a Context Source
/// Registration Subscription with `datasetId` a is notified `newlyMatching` of the first and the third, one with
/// `datasetId` `@none` of the third alone, and one with no `datasetId` of all three
/// Clause: 5.12, 4.3.6.1, 5.2.12, 5.2.9, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_css_ntf_9014_a_dataset_id_on_a_csource_subscription_notifies_the_registrations_that_list_it_or_list_none() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_a = namespace.dataset_id("Dataset", "a")?;
    let dataset_b = namespace.dataset_id("Dataset", "b")?;
    let none = "@none".parse::<DatasetId>()?;
    let registration_a_id = namespace.registration_id("a")?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_unlisted_id = namespace.registration_id("unlisted")?;
    let subscription_ids = [namespace.subscription_id(1)?, namespace.subscription_id(2)?, namespace.subscription_id(3)?];
    let _cleanup = Cleanup::of(&broker)
        .registration_subscription(&subscription_ids[0])
        .registration_subscription(&subscription_ids[1])
        .registration_subscription(&subscription_ids[2])
        .registration(&registration_a_id)
        .registration(&registration_b_id)
        .registration(&registration_unlisted_id);
    for (index, dataset_ids) in [Some(vec![dataset_a.clone()]), Some(vec![none]), None].into_iter().enumerate() {
        let number = u32::try_from(index + 1)?;
        let subscription = Subscription::registration_builder()
            .id(subscription_ids[index].clone())
            .entities([vehicle_type.as_str()])
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(number)?).accept(NgsiLdMediaType::Json).build()?).build()?);
        let subscription = match dataset_ids {
            Some(dataset_ids) => subscription.dataset_ids(dataset_ids).build()?,
            None => subscription.build()?,
        };
        broker.registration_subscriptions().create(&subscription).send().await?;
    }
    let vehicles = RegistrationInfo::builder()
        .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
        .build()?;
    // The registration listing the other dataset goes first, so a wrong notification of it precedes the awaited ones.
    for (id, dataset_ids) in [
        (&registration_b_id, Some(vec![dataset_b])),
        (&registration_a_id, Some(vec![dataset_a])),
        (&registration_unlisted_id, None),
    ] {
        let registration = CSourceRegistration::builder(source.endpoint("copies")?)
            .id(id.clone())
            .information([vehicles.clone()])
            .mode(RegistrationMode::Inclusive);
        let registration = match dataset_ids {
            Some(dataset_ids) => registration.dataset_ids(dataset_ids).build()?,
            None => registration.build()?,
        };
        broker.registrations().register(&registration).send().await?;
    }
    // Clause 5.11.2.4 lets one notification carry several registrations, and a Subscription notified initially after
    // the registrations committed carries them together, so each registration is awaited by its identifier.
    for (number, awaited) in [
        (3, vec![&registration_b_id, &registration_a_id, &registration_unlisted_id]),
        (1, vec![&registration_a_id, &registration_unlisted_id]),
        (2, vec![&registration_unlisted_id]),
    ] {
        for registration_id in awaited {
            let carrying = BodyPattern::Selects(format!("$.data[?(@.id == '{registration_id}')]").parse()?);
            sink.wait_for_at_least_where(number, &[carrying], 1).await?;
        }
    }
    expect_that!(
        notified_registrations(&sink, 1).await?,
        unordered_elements_are![eq(&registration_a_id), eq(&registration_unlisted_id)]
    );
    expect_that!(notified_registrations(&sink, 2).await?, elements_are![eq(&registration_unlisted_id)]);
    expect_that!(
        notified_registrations(&sink, 3).await?,
        unordered_elements_are![eq(&registration_a_id), eq(&registration_b_id), eq(&registration_unlisted_id)]
    );
    Ok(())
}
