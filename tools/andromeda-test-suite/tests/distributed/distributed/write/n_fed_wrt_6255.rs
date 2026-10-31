//! Partial update partial success is an update result naming the registration.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    report_matcher::not_updated,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Attribute, AttributeName, Entity, Property},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Partial Attribute Update of speed under two redirect registrations, one not advertising `updateAttrs`,
/// answers 207 with an `UpdateResult` whose `notUpdated` names speed
/// Clause: 5.6.4.4, 5.2.18, 5.2.19, 6.7.3.1
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6255_partial_update_partial_success_is_an_update_result_naming_the_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([
            DistributedOperation::UpdateAttrs,
            DistributedOperation::DeleteAttrs,
            DistributedOperation::RetrieveEntity,
        ])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    source
        .answer(
            Method::PATCH,
            &format!("/s1/ngsi-ld/v1/entities/{vehicle_id}/attrs/speed"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let patch = Attribute::from(Property::builder(99).build()?).into_patch();
    let report = broker
        .entities()
        .update_attribute(&vehicle_id, &"speed".parse::<AttributeName>()?, patch)
        .send()
        .await?
        .partial()?;
    expect_that!(&report.not_updated, len(eq(1)));
    expect_that!(&report.not_updated, nth(0, not_updated().attribute("speed")));
    Ok(())
}
