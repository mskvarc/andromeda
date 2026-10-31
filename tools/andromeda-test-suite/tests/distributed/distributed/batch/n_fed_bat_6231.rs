//! Delete falls back to one delete per id when the batch operation is not advertised.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode};

/// Pins: Batch Entity Delete under a redirect registration advertising `deleteEntity` alone forwards one delete per
/// id
/// Clause: 5.6.10.4
#[gtest]
#[tokio::test]
async fn n_fed_bat_6231_delete_falls_back_to_one_delete_per_id_when_the_batch_operation_is_not_advertised() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::DeleteEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::DELETE,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    source
        .answer(
            Method::DELETE,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_2_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let ids = [vehicle_id.clone(), vehicle_2_id.clone()];
    broker.entities().batch_delete(&ids).send().await?.done()?;
    let forwarded = source.received_on(Method::DELETE, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source.received_on(Method::DELETE, &format!("/a/ngsi-ld/v1/entities/{vehicle_2_id}")).await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_3 = source.received_on(Method::POST, "/a/ngsi-ld/v1/entityOperations/delete").await?;
    expect_that!(forwarded_3, len(eq(0)));
    Ok(())
}
