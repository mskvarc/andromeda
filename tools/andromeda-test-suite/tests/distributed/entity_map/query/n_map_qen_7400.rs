//! A named map fixes the sources consulted and an unknown map is made anew.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_matcher::present,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::Entity,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities with `entityMap` true answers 201 with an `NGSILD-EntityMap` header, the same query naming
/// that map answers 200 with the same header and never reaches a source registered after the map was made, and a
/// query naming an unknown map answers 201 with a new map and reaches that source
/// Clause: 5.7.2.4, 5.5.14, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_7400_a_named_map_fixes_the_sources_consulted_and_an_unknown_map_is_made_anew() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [Entity::output_builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?];
    source
        .answer(Method::GET, "/s1/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
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
    expect_that!(page.items(), len(eq(1)));
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let source_entities_2 = [];
    source
        .answer(Method::GET, "/s2/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
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
    let forwarded_request = SourceRequest::matching(Method::GET, "/s1/ngsi-ld/v1/entities");
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(2)));
    let forwarded_request_2 = SourceRequest::matching(Method::GET, "/s2/.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    let entity_map_request_3 = EntityMapRequest::reuse(NgsiLdEntityMap::try_new(IriRefBuf::new(format!("urn:ngsi-ld:EntityMap:{namespace}-bogus"))?)?);
    let page_3 = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request_3)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page_3.entity_map(), present());
    let forwarded_request_3 = SourceRequest::any_matching("/s2/.*");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(ge(1)));
    Ok(())
}
