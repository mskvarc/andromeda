//! Inclusive replace forwards the entity and replaces locally keeping created at.

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
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::Entity,
    query::Locality,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Replace Entity under an inclusive registration answers 204, forwards one replace carrying `brandName` and
/// speed, replaces the local Entity, and keeps its `createdAt`
/// Clause: 5.6.18.4, 4.3.6.4, 4.8
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6215_inclusive_replace_forwards_the_entity_and_replaces_locally_keeping_created_at() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .property("speed", 42)
        .build()?;
    broker.entities().create(&vehicle).locality(Locality::Local).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    let created = system_time::of_entity(entity, SystemMember::CreatedAt)?;
    expect_that!(system_time::of_entity(entity, SystemMember::CreatedAt).ok(), some(anything()));
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .mode(RegistrationMode::Inclusive)
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
    let vehicle_replacement = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "BMW")
        .property("speed", 99)
        .build()?;
    broker.entities().replace(&vehicle_replacement).send().await?.done()?;
    let forwarded = source.received_on(Method::PUT, &format!("/a/ngsi-ld/v1/entities/{vehicle_id}")).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let forwarded_entity = forwarded_body::entity(forwarded_one)?;
    expect_that!(&forwarded_entity, has_attribute("speed", any_attribute().value(99)));
    expect_that!(&forwarded_entity, has_attribute("brandName", any_attribute().value("BMW")));
    let retrieved_2 = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .locality(Locality::Local)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(system_time::of_entity(entity_2, SystemMember::CreatedAt)?, eq(created));
    expect_that!(entity_2, has_attribute("brandName", any_attribute().value("BMW")));
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(99)));
    Ok(())
}
