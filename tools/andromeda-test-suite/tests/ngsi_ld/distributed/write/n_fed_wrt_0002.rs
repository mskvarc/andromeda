//! A create existing locally and at the source is already exists.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, problem::problem, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Entity, ErrorType, Relationship},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Create Entity of an Entity held locally whose inclusive source answers the forwarded create with 409 fails
/// as a whole: 409 `AlreadyExists`, after one forwarded create
/// Clause: 5.6.1.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_wrt_0002_a_create_existing_locally_and_at_the_source_is_already_exists() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([
            DistributedOperation::CreateEntity,
            DistributedOperation::UpdateEntity,
            DistributedOperation::DeleteEntity,
            DistributedOperation::MergeEntity,
            DistributedOperation::ReplaceEntity,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::CONFLICT))
        .await?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&vehicle_2).send().await;
    expect_that!(&answer, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    let forwarded = source.received_on(Method::POST, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
