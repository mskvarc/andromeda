//! Purge under an entity map is forwarded only to the registrations the map names.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_head,
    forwarded_query::query_values,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Purge Entities by type with `NGSILD-EntityMap` naming a map minted over one registration answers 204 and
/// forwards a purge only to that registration, carrying the linked map header and the mapped entity id, and none to
/// a registration matching by type that the map does not name
/// Clause: 5.14.1, 5.6.21.4, 6.4.3.3, 5.5.14
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6252_purge_under_an_entity_map_is_forwarded_only_to_the_registrations_the_map_names() -> Result<()> {
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
        .property("speed", 42)
        .build()?];
    source
        .answer(Method::GET, "/in/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    source
        .answer(Method::DELETE, "/in/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    source
        .answer(Method::DELETE, "/out/ngsi-ld/v1/entities", source_answer::status(StatusCode::NO_CONTENT))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("in")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity, DistributedOperation::PurgeEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
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
    let map_header = page.entity_map().valid().or_fail()?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("out")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity, DistributedOperation::PurgeEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    broker
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .entity_map(map_header)
        .send()
        .await?
        .done()?;
    let forwarded = source.received_on(Method::DELETE, "/in/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(forwarded_head::head(forwarded_one).typed::<NgsiLdEntityMap>().valid(), some(anything()));
    expect_that!(query_values(forwarded_one, "id"), contains(eq(&vehicle_id.to_string())));
    let forwarded_2 = source.received_on(Method::DELETE, "/out/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
