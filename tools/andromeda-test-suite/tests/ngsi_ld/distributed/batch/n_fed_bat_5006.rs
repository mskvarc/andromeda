//! A batch deletion reaches a registration of a type that names no identifier.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::EntityId,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Delete of two ids the broker does not hold, under a redirect registration of the Vehicle type
/// naming neither an id nor an id pattern and advertising `deleteBatch`, forwards its source one batch holding both
/// ids
/// Clause: 5.6.10.4, 5.12
/// Tenant: private (a deletion carries no type, so a registration of a type alone matches the batch deletions of
/// every other test in the tenant)
#[gtest]
#[tokio::test]
async fn n_fed_bat_5006_a_batch_deletion_reaches_a_registration_of_a_type_that_names_no_identifier() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::DeleteBatch, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The registration is deleted when the test ends. A forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::POST,
            "/s1/ngsi-ld/v1/entityOperations/delete",
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let ids = [vehicle_id.clone(), vehicle_2_id.clone()];
    broker.entities().batch_delete(&ids).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/s1/ngsi-ld/v1/entityOperations/delete").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(
        serde_json::from_slice::<Vec<EntityId>>(&forwarded_one.body)?,
        elements_are![eq(&vehicle_id), eq(&vehicle_2_id)]
    );
    Ok(())
}
