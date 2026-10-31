//! Delete of an entity held only by a registration is forwarded.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode};

/// Pins: Delete Entity for an id the broker does not hold, matched by a redirect registration advertising
/// `deleteEntity`, answers 204 and forwards one delete
/// Clause: 5.6.6.4, 4.3.6.3
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6242_delete_of_an_entity_held_only_by_a_registration_is_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id(vehicle_id.clone())])
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
    broker.entities().delete(&vehicle_id).send().await?.done()?;
    let forwarded = source.received_on(Method::DELETE, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
