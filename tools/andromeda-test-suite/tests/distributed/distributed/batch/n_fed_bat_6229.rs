//! Delete forwards each redirect source only the ids it registers.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::EntityId,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Delete of two ids, each registered by its own redirect registration advertising `deleteBatch`,
/// forwards each source one batch holding only its own id
/// Clause: 5.6.10.4, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_bat_6229_delete_forwards_each_redirect_source_only_the_ids_it_registers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::DeleteBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::DeleteBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/delete",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    source
        .answer(
            Method::POST,
            "/s2/ngsi-ld/v1/entityOperations/delete",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let ids = [vehicle_id.clone(), vehicle_2_id.clone()];
    broker.entities().batch_delete(&ids).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/delete").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(serde_json::from_slice::<Vec<EntityId>>(&forwarded_one.body)?, elements_are![eq(&vehicle_id)]);
    let forwarded_2 = source.received_on(Method::POST, "/s2/ngsi-ld/v1/entityOperations/delete").await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_one_2 = forwarded_2.first().or_fail()?;
    expect_that!(
        serde_json::from_slice::<Vec<EntityId>>(&forwarded_one_2.body)?,
        elements_are![eq(&vehicle_2_id)]
    );
    Ok(())
}
