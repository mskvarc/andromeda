//! A periodic Subscription's copy meets the Subscription data type.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{accept_copies, placed_copy},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: over an inclusive `federationOps` registration of the type naming the Property `speed`, a Subscription by
/// type with `timeInterval` 60 and no `watchedAttributes` places a copy with `timeInterval` 60 and no
/// `watchedAttributes`, which Table 5.2.12-1 gives no place beside it
/// Clause: 5.8.1.4, Table 5.2.12-1, 4.3.6.1
/// Recorded: no member of a periodic copy narrows the Attributes its source notifies, so this broker keeps only the
/// Attributes the registration names of each Entity the source relays
#[gtest]
#[tokio::test]
async fn n_sub_cre_6818_a_periodic_subscription_copy_meets_the_subscription_data_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    accept_copies(&source, "s1").await?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .time_interval(60)
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let _cleanup = Cleanup::of(&broker).registration(&registration_id).subscription(&subscription_id);
    broker.subscriptions().create(&subscription).send().await?;
    let placed = placed_copy(&source, "s1", &vehicle_type).await?;
    expect_that!(placed.body.as_slice(), json_equals("$.timeInterval", 60));
    expect_that!(placed.body.as_slice(), json_absent("$.watchedAttributes"));
    Ok(())
}
