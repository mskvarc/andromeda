//! An auxiliary registration never receives an append.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment},
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Append Entity Attributes of speed to an Entity held locally under an auxiliary registration answers 204,
/// forwards nothing, and the local speed takes the new value
/// Clause: 4.3.6.2, 5.6.3.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_mod_6409_an_auxiliary_registration_never_receives_an_append() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::RetrieveEntity, DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::POST,
            &format!("/ngsi-ld/v1/entities/{vehicle_id}/attrs"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let fragment = EntityFragment::builder().property("speed", 20).build()?;
    broker.entities().append_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, &format!("/ngsi-ld/v1/entities/{vehicle_id}/attrs")).await?;
    expect_that!(forwarded, len(eq(0)));
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(20)));
    Ok(())
}
