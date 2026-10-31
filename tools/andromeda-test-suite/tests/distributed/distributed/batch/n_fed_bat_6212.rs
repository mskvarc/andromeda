//! Upsert updates an entity the source reports as existing in update mode.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    forwarded_body,
    namespace::Namespace,
    outcome::Done,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::operation::UpsertMode,
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Batch Entity Upsert with options update under a redirect registration advertising `createEntity` and
/// `updateEntity`, whose source answers the create with 409 `AlreadyExists`, forwards one update of the entity
/// attributes
/// Clause: 5.6.8.4
#[gtest]
#[tokio::test]
async fn n_fed_bat_6212_upsert_updates_an_entity_the_source_reports_as_existing_in_update_mode() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Redirect)
        .operations([
            DistributedOperation::CreateEntity,
            DistributedOperation::UpdateEntity,
            DistributedOperation::RetrieveEntity,
        ])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/a/ngsi-ld/v1/entities", source_answer::status(StatusCode::CONFLICT))
        .await?;
    source
        .answer(
            Method::PATCH,
            &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs"),
            source_answer::status(StatusCode::NO_CONTENT),
        )
        .await?;
    let entities = [Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?];
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let forwarded = source.received_on(Method::POST, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_2 = source.received_on(Method::PATCH, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}/attrs")).await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_one = forwarded_2.first().or_fail()?;
    let forwarded_entity = forwarded_body::fragment(forwarded_one)?;
    expect_that!(&forwarded_entity, has_attribute("speed", any_attribute().value(1)));
    Ok(())
}
