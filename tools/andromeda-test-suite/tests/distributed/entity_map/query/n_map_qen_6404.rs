//! A query over a map contacts only the source holding the entity.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_matcher::present,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type and id naming an `EntityMap` that maps the Entity to one of two sources is
/// forwarded again to that source and never again to the other
/// Clause: 5.12, 4.3.6.7, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_map_qen_6404_a_query_over_a_map_contacts_only_the_source_holding_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 33)
        .build()?];
    source
        .answer(Method::GET, "/1/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [];
    source
        .answer(Method::GET, "/2/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
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
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let forwarded = source.received_on(Method::GET, "/1/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(2)));
    let forwarded_2 = source.received_on(Method::GET, "/2/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
