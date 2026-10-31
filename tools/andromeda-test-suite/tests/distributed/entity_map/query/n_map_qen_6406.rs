//! A query over a map passes the map the source returned to that source.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    header_matcher::present,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type with `entityMap` true over a source answering an `NGSILD-EntityMap` header, then
/// naming the broker's map, answers 200 with the same map and is forwarded the second time with the source's map
/// location
/// Clause: 4.3.6.7, 5.7.2.4, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_6406_a_query_over_a_map_passes_the_map_the_source_returned_to_that_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let source_entities = [Entity::output_builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 25)
        .build()?];
    source
        .answer(
            Method::GET,
            "/1/ngsi-ld/v1/entities",
            source_answer::entities(&source_entities).insert_header("ngsild-entitymap", format!("urn:ngsi-ld:EntityMap:{namespace}-source")),
        )
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
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let headed = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    let page_2 = headed.outcome;
    expect_that!(headed.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map)));
    expect_that!(page_2.items(), len(eq(1)));
    let forwarded = source.received_on(Method::GET, "/1/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(2)));
    let forwarded_request = SourceRequest::new(Method::GET, "/1/ngsi-ld/v1/entities").header(HeaderCondition::Contains(
        HeaderName::from_static("ngsild-entitymap"),
        format!("urn:ngsi-ld:EntityMap:{namespace}-source"),
    ));
    let forwarded_2 = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded_2, len(eq(1)));
    Ok(())
}
