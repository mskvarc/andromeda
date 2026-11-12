//! A batch reaches a registration whose information names no entities.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::{has_attribute, lacks_attribute},
    forwarded_body,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Creation of a Vehicle and a Boat, each with `brandName` and speed, under a redirect
/// registration of speed whose information names no entities and which advertises `createBatch`, forwards its source
/// one batch holding both Entities with speed alone
/// Clause: 5.6.7.4, 5.12, 5.2.10
/// Tenant: private (a registration naming no entities matches the batches of every other test in the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_bat_5005_a_batch_reaches_a_registration_whose_information_names_no_entities() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let boat_id = namespace.urn("Boat", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().property_names(["speed"]).build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The registration is deleted when the test ends. A forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(namespace.entity_type("Vehicle")?)
            .property("brandName", "Mercedes")
            .property("speed", 42)
            .build()?,
        Entity::builder()
            .id(boat_id.clone())
            .entity_type(namespace.entity_type("Boat")?)
            .property("brandName", "Riva")
            .property("speed", 17)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(
        forwarded_body::entities(forwarded_one)?,
        all!(
            len(eq(2)),
            nth(
                0,
                all!(
                    has_id(&vehicle_id),
                    has_attribute("speed", any_attribute().value(42)),
                    lacks_attribute("brandName")
                )
            ),
            nth(
                1,
                all!(
                    has_id(&boat_id),
                    has_attribute("speed", any_attribute().value(17)),
                    lacks_attribute("brandName")
                )
            )
        )
    );
    Ok(())
}
