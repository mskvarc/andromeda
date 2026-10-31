//! A named map fixes the sources consulted and an unknown map is made anew.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    source_answer,
    source_request::SourceRequest,
    temporal_map_retrieval::retrieve_making_map,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::{Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution of an Entity with `entityMap` true answers an `NGSILD-EntityMap` header, the
/// retrieval naming that map answers 200 with the same header and never reaches a source registered after the map
/// was made, and a retrieval naming an unknown map reaches that source
/// Clause: 5.7.3.4, 5.5.14, 6.19.3.1
/// Recorded: clause 5.7.3.4 has Retrieve Temporal Evolution create and reuse an `EntityMap` but the HTTP binding of
/// Retrieve Temporal Evolution lists neither the `NGSILD-EntityMap` header nor a 201 answer; Andromeda answers 201
/// with the header when it makes a map, as it does for Query Temporal Evolution of Entities, and 200 when it reuses
/// one
#[gtest]
#[tokio::test]
async fn o_map_qte_7400_a_named_map_fixes_the_sources_consulted_and_an_unknown_map_is_made_anew() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(namespace.registration_id(1)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let source_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s1/ngsi-ld/v1/temporal/entities/{vehicle_id}"),
            source_answer::temporal_entity(&source_evolution),
        )
        .await?;
    let entity_map_request = EntityMapRequest::returned();
    let window = temporal_query::before("2030-01-01T00:00:00Z".parse()?).build()?;
    let made = retrieve_making_map(&broker, &vehicle_id, &window, &entity_map_request).await?;
    expect_that!(&made.evolution, has_evolution("speed", nth(0, any_attribute().value(50))));
    let map = made.map;
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(namespace.registration_id(2)?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveTemporal])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let source_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s2/ngsi-ld/v1/temporal/entities/{vehicle_id}"),
            source_answer::temporal_entity(&source_evolution_2),
        )
        .await?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let headed = broker
        .with_heads()
        .with_accept(Accept::any())
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(headed.status, eq(StatusCode::OK));
    expect_that!(headed.head.typed::<NgsiLdEntityMap>().valid(), some(eq(&map)));
    let forwarded_request = SourceRequest::matching(Method::GET, &format!("/s1/ngsi-ld/v1/temporal/entities/{vehicle_id}"));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(2)));
    let forwarded_request_2 = SourceRequest::matching(Method::GET, "/s2/.*");
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(0)));
    let entity_map_request_3 = EntityMapRequest::reuse(NgsiLdEntityMap::try_new(IriRefBuf::new(format!("urn:ngsi-ld:EntityMap:{namespace}-bogus"))?)?);
    let made_3 = retrieve_making_map(&broker, &vehicle_id, &window, &entity_map_request_3).await?;
    expect_that!(made_3.map.id(), some(anything()));
    let forwarded_request_3 = SourceRequest::any_matching("/s2/.*");
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(ge(1)));
    Ok(())
}
