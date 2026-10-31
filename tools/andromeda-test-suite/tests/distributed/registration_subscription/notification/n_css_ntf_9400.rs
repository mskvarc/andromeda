//! A context source subscription's csf reads the operations array by membership.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, notification_sink::NotificationSink};
use googletest::prelude::*;
use ngsi_ld::{
    model::{NgsiLdMediaType, Output},
    query::q::Query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, TriggerReason},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Context Source Registration Subscription with `csf` `operations=="queryEntity"` is notified of the
/// registration whose `operations` lists `queryEntity` among others, and not of the one whose `operations` does not
/// list it
/// Clause: 4.9, 5.11.7, 5.12
#[gtest]
#[tokio::test]
async fn n_css_ntf_9400_a_context_source_subscriptions_csf_reads_the_operations_array_by_membership() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let origin = SuiteConfig::get()?.unreachable_origin();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicles_id = namespace.registration_id("vehicles")?;
    let buildings_id = namespace.registration_id("buildings")?;
    let _cleanup = Cleanup::of(&broker)
        .registration_subscription(&subscription_id)
        .registration(&vehicles_id)
        .registration(&buildings_id);
    let subscription = Subscription::registration_builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .csf(r#"operations=="queryEntity""#.parse::<Query>()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    for (id, operations) in [
        (&buildings_id, vec![DistributedOperation::RetrieveEntity]),
        (&vehicles_id, vec![DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity]),
    ] {
        let registration = CSourceRegistration::builder(origin.as_str())
            .id(id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .build()?])
            .operations(operations)
            .build()?;
        broker.registrations().register(&registration).send().await?;
    }
    sink.wait_for_at_least(1, 1).await?;
    let notifications = sink.received(1).await?;
    expect_that!(notifications, len(eq(1)));
    let notification = notifications.first().or_fail()?.registrations()?;
    expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
    expect_that!(notification.data(), len(eq(1)));
    expect_that!(notification.data().first().map(CSourceRegistration::<Output>::id), some(eq(&vehicles_id)));
    Ok(())
}
