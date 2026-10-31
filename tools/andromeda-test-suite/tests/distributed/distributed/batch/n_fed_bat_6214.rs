//! Upsert falls back to one replace per entity when only replace is advertised.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Upsert in replace mode under a redirect registration advertising `replaceEntity` alone
/// forwards one replace per entity
/// Clause: 5.6.8.4
#[gtest]
#[tokio::test]
async fn n_fed_bat_6214_upsert_falls_back_to_one_replace_per_entity_when_only_replace_is_advertised() -> Result<()> {
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
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([DistributedOperation::ReplaceEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::PUT,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    source
        .answer(
            Method::PUT,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_2_id}"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let forwarded = source.received_on(Method::PUT, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source.received_on(Method::PUT, &format!("/a/ngsi-ld/v1/entities/{vehicle_2_id}")).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
