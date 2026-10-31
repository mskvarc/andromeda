//! An entity a source returns without system attributes is answered without them.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    source_answer,
    system_time::{self, SystemMember},
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::SystemAttributes},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with options `sysAttrs` over a source returning an Entity without `createdAt` and
/// `modifiedAt` answers 200 with that Entity and no `createdAt`
/// Clause: 5.7.2.4, 4.5.9
/// Recorded: the specification does not say what a broker answers for a source Entity that carries no system
/// attributes; Andromeda adds none of its own
#[gtest]
#[tokio::test]
async fn o_fed_qry_6410_an_entity_a_source_returns_without_system_attributes_is_answered_without_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let item = page.items().first().or_fail()?;
    expect_that!(system_time::of_entity(item, SystemMember::CreatedAt).ok(), none());
    expect_that!(page.items(), nth(0, has_attribute("speed", any_attribute().value(55))));
    Ok(())
}
