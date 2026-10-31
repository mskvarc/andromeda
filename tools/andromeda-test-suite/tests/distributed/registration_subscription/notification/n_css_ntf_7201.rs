//! An accept member of ld json makes the notification application ld json.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    namespace::Namespace,
    notification_sink::NotificationSink,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: A Context Source Registration matching a csource subscription whose endpoint.accept is application/ld+json
/// is notified with the content type application/ld+json
/// Clause: 6.3.9, 5.11.7
#[gtest]
#[tokio::test]
async fn n_css_ntf_7201_an_accept_member_of_ld_json_makes_the_notification_application_ld_json() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let subscription_id = SubscriptionId::from(namespace.uri("ContextSourceRegistrationSubscription", 1)?);
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let subscription = Subscription::registration_builder()
        .id(subscription_id)
        .entities([vehicle_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::JsonLd).build()?).build()?)
        .build()?;
    broker.registration_subscriptions().create(&subscription).send().await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let fields = [HeaderCondition::Contains(
        HeaderName::from_static("content-type"),
        "application/ld+json".to_owned(),
    )];
    sink.wait_for_at_least_matching(1, &[], &fields, 1).await?;
    Ok(())
}
