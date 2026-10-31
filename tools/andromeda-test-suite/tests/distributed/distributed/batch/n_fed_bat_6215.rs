//! Upsert falls back to one update per entity when only update is advertised.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, outcome::Done, source_answer};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::UpsertMode,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Upsert with options update under a redirect registration advertising `updateEntity` alone
/// forwards one attribute update per entity
/// Clause: 5.6.8.4
#[gtest]
#[tokio::test]
async fn n_fed_bat_6215_upsert_falls_back_to_one_update_per_entity_when_only_update_is_advertised() -> Result<()> {
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
        .operations([DistributedOperation::UpdateEntity, DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_2_id}/attrs"),
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
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let forwarded = source.received_on(Method::PATCH, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs")).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source
        .received_on(Method::PATCH, &format!("/a/ngsi-ld/v1/entities/{vehicle_2_id}/attrs"))
        .await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
