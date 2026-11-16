//! A relayed entity keeps only the Attributes its source's registration names.

use andromeda_test_suite::{
    body_json::{json_absent, json_count, json_equals},
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    core_context::core_context,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{accept_copies, notification_body, placed_copy_id, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: over an inclusive `federationOps` registration of the type naming the Property `speed`, a Subscription by
/// type is notified of an Entity the source relays holding `speed` and `fuel` with `speed` alone, and not of one
/// holding `fuel` alone
/// Clause: 4.3.6.1, 5.8.1.4, 5.8.6
/// Recorded: the copy carries no `notification.attributes` (clause 5.8.1.4), so the source notifies every Attribute it
/// holds, and this broker keeps only those the registration names
#[gtest]
#[tokio::test]
async fn n_ntf_pay_6823_a_relayed_entity_keeps_only_the_attributes_its_registration_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed = namespace.attribute("speed")?;
    let fuel = namespace.attribute("fuel")?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    accept_copies(&source, "s1").await?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names([speed.to_string()])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?).build()?)
        .build()?;
    let _cleanup = Cleanup::of(&broker).registration(&registration_id).subscription(&subscription_id);
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let both_id = namespace.urn("Vehicle", "both")?;
    let both = Entity::output_builder()
        .id(both_id.clone())
        .entity_type(vehicle_type.clone())
        .property(speed.clone(), 40)
        .property(fuel.clone(), 12)
        .build()?;
    let fuel_only = Entity::output_builder()
        .id(namespace.urn("Vehicle", "fuel")?)
        .entity_type(vehicle_type)
        .property(fuel.clone(), 13)
        .build()?;
    let body = notification_body(&namespace, &copy_id, vec![both, fuel_only], core_context()?)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    expect_that!(&received.body, json_count("$.data", 1));
    expect_that!(&received.body, json_equals("$.data[0].id", both_id.to_string()));
    expect_that!(&received.body, json_equals(&format!("$.data[0]['{speed}'].value"), 40));
    expect_that!(&received.body, json_absent(&format!("$.data[0]['{fuel}']")));
    Ok(())
}
