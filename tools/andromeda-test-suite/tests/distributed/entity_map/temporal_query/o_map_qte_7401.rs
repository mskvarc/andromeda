//! The map a source answers is linked and passed on by a retrieval.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
    temporal_map_retrieval::retrieve_making_map,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::{Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMapId, RegistrationInfo},
};

/// Pins: Retrieve Temporal Evolution of an Entity with `entityMap` true over a source answering its own
/// `NGSILD-EntityMap` records that map in the `linkedMaps` of the new `EntityMap` under the registration, and a
/// retrieval naming the new map forwards the source map to the source
/// Clause: 5.7.3.4, 5.5.14, 6.3.15
/// Recorded: clause 5.7.3.4 has Retrieve Temporal Evolution create and reuse an `EntityMap` but the HTTP binding of
/// Retrieve Temporal Evolution lists neither the `NGSILD-EntityMap` header nor a 201 answer; Andromeda answers 201
/// with the header when it makes a map, as it does for Query Temporal Evolution of Entities, and 200 when it reuses
/// one
#[gtest]
#[tokio::test]
async fn o_map_qte_7401_the_map_a_source_answers_is_linked_and_passed_on_by_a_retrieval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let linked_id = format!("urn:ngsi-ld:EntityMap:{namespace}-linked");
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
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
            source_answer::temporal_entity(&source_evolution).insert_header("NGSILD-EntityMap", linked_id.as_str()),
        )
        .await?;
    let entity_map_request = EntityMapRequest::returned();
    let window = temporal_query::before("2030-01-01T00:00:00Z".parse()?).build()?;
    let made = retrieve_making_map(&broker, &vehicle_id, &window, &entity_map_request).await?;
    let map = made.map;
    let map_id = map.id().or_fail()?;
    let stored = broker.with_accept(Accept::any()).entity_maps().retrieve(&map_id).send().await?;
    let linked_map_id = linked_id.parse::<EntityMapId>()?;
    expect_that!(stored.linked_maps.get(&registration_id), some(eq(&linked_map_id)));
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
    let passed_on = SourceRequest::matching(Method::GET, "/s1/.*").header(HeaderCondition::Is(HeaderName::from_static("ngsild-entitymap"), linked_id.clone()));
    let forwarded = source.received_matching(&passed_on).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
