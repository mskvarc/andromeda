//! A registration that stops naming Attributes widens the copy it narrowed.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    relayed_copy::{accept_copies, placed_copy},
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationFragment, RegistrationInfo, RegistrationMode},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: over an inclusive `federationOps` registration of the type naming the Property `speed`, a Subscription by
/// type naming no `watchedAttributes` places a copy watching `speed`, and an update of the registration that names no
/// Attribute patches the copy with `watchedAttributes` written as the NGSI-LD Null beside its selector
/// Clause: 5.8.1.4, 5.5.8, 5.2.12, 4.3.6.1, 5.11.2.4
/// Recorded: the copy watches what the registration names while the Subscription names nothing, so it follows the
/// registration as well as the Subscription, and only a deletion widens it again at the source
#[gtest]
#[tokio::test]
async fn n_sub_upd_6810_a_registration_that_stops_naming_attributes_widens_the_copy_it_narrowed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let speed = namespace.attribute("speed")?;
    let registration_id = namespace.registration_id(1)?;
    let subscription_id = namespace.subscription_id(1)?;
    let vehicles = EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?);
    accept_copies(&source, "s1").await?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([vehicles.clone()])
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
    let placed = placed_copy(&source, "s1", &vehicle_type).await?;
    // The copy names its terms expanded under the core context, so the body is read for them.
    expect_that!(
        placed.body.as_slice(),
        json_equals("$.watchedAttributes", vec![format!("https://uri.etsi.org/ngsi-ld/default-context/{speed}")])
    );
    let unnamed = RegistrationFragment::builder()
        .information([RegistrationInfo::builder().entities([vehicles]).build()?])
        .build()?;
    broker.registrations().update(&registration_id, &unnamed).send().await?;
    let patched = source
        .wait_for_matching(&SourceRequest::matching(Method::PATCH, "/s1/ngsi-ld/v1/subscriptions/.+"), 1)
        .await?;
    expect_that!(patched, len(eq(1)));
    let patch = patched.first().or_fail()?.body.as_slice();
    // Table 5.2.12-1 has `watchedAttributes` an array, so its NGSI-LD Null is the array of one Null (clause 4.5.0).
    expect_that!(patch, json_equals("$.watchedAttributes", vec!["urn:ngsi-ld:null"]));
    expect_that!(
        patch,
        json_equals("$.entities[0].type", format!("https://uri.etsi.org/ngsi-ld/default-context/{vehicle_type}"))
    );
    Ok(())
}
