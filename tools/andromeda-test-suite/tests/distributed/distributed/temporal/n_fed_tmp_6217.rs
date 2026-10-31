//! Inclusive delete of a temporal evolution held only by the source is forwarded.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo};

/// Pins: Delete Temporal Evolution of an Entity with no local Temporal Evolution under an inclusive registration
/// advertising `deleteTemporal` answers 204 and forwards one delete to the entity path
/// Clause: 5.6.16.4, 4.3.6.4
#[gtest]
#[tokio::test]
async fn n_fed_tmp_6217_inclusive_delete_of_a_temporal_evolution_held_only_by_the_source_is_forwarded() -> Result<()> {
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
        .operations([DistributedOperation::DeleteTemporal, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::DELETE,
            &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    broker.temporal().delete(&vehicle_id).send().await?;
    let forwarded = source
        .received_on(Method::DELETE, &format!("/a/ngsi-ld/v1/temporal/entities/{vehicle_id}"))
        .await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
