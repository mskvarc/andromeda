//! An entity map maps the local entity to none and the forwarded one to its registration.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_matcher::present,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType, Relationship},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with `entityMap` true under an auxiliary registration answers 201, and the
/// `EntityMap` it names maps the locally held Entity to @none and the Entity only the source returned to that
/// registration
/// Clause: 5.7.2.4, 5.2.39, 5.5.14
#[gtest]
#[tokio::test]
async fn n_fed_qry_0008_an_entity_map_maps_the_local_entity_to_none_and_the_forwarded_one_to_its_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let off_street_parking_downtown1_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(off_street_parking_downtown1_id.clone()).observed_at("2017-07-29T12:00:04Z".parse()?),
        )
        .build()?];
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query_containing("type", format!("Vehicle_{namespace}")),
            source_answer::entities(&source_entities),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Auxiliary)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(map_2.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(
        map_2.entity_map.get(&vehicle_2_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
