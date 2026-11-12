//! A batch reaches a registration created in a tenant that held none, and not once it is deleted.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, outcome::Done, problem::problem, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::{Entity, ErrorType, Input},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: in a tenant holding no registration, Batch Entity Creation answers 201 and forwards nothing; once a redirect
/// registration of speed advertising `createBatch` is created, a second creation of it answers 409 `AlreadyExists`
/// and a deletion of an unknown registration answers 404 `ResourceNotFound`, the next Batch Entity Creation forwards
/// its source one batch holding the Vehicle; once the registration is deleted, the next one forwards nothing
/// Clause: 5.6.7.4, 5.9.2, 5.9.5, 5.12
/// Tenant: private (it needs a tenant that holds no registration, and the default tenant holds the registrations of
/// other tests)
#[gtest]
#[tokio::test]
async fn n_fed_bat_5004_a_batch_reaches_a_registration_created_in_a_tenant_that_held_none_and_not_once_it_is_deleted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/create",
            source_answer::status(StatusCode::CREATED),
        )
        .await?;
    // The Vehicles carry `brandName`, which stays local whatever registration exists. The first carries no speed,
    // so the registration of speed created after it claims nothing the broker holds (clause 5.9.2.4).
    let vehicle = |number: u32| -> Result<Entity<Input>> {
        Ok(Entity::builder()
            .id(namespace.urn("Vehicle", number)?)
            .entity_type(vehicle_type.clone())
            .property("brandName", "Mercedes")
            .property("speed", number)
            .build()?)
    };
    let first = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .build()?;
    broker.entities().batch_create(&[first]).send().await?.done()?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::CreateBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let duplicate = broker.with_responses().registrations().register(&registration).send().await;
    expect_that!(&duplicate, problem(StatusCode::CONFLICT, ErrorType::AlreadyExists));
    let unknown = broker.with_responses().registrations().delete(&namespace.registration_id(2)?).send().await;
    expect_that!(&unknown, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    broker.entities().batch_create(&[vehicle(2)?]).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    broker.entities().batch_create(&[vehicle(3)?]).send().await?.done()?;
    let forwarded_after = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/create").await?;
    expect_that!(forwarded_after, len(eq(1)));
    Ok(())
}
