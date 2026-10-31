//! The map a source answers is linked and passed on by a retrieval.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_condition::HeaderCondition,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, Method};
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMapId, RegistrationInfo},
};

/// Pins: Retrieve Entity with `entityMap` true over a source answering its own `NGSILD-EntityMap` records that map
/// in the `linkedMaps` of the new `EntityMap` under the registration, and a retrieval naming the new map forwards
/// the source map to the source
/// Clause: 5.7.1.4, 5.5.14, 6.3.15
#[gtest]
#[tokio::test]
async fn n_map_qen_7403_the_map_a_source_answers_is_linked_and_passed_on_by_a_retrieval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s1/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::entity(&source_entity).insert_header("ngsild-entitymap", format!("urn:ngsi-ld:EntityMap:{namespace}-linked")),
        )
        .await?;
    let entity_map_request = EntityMapRequest::returned();
    let retrieved = broker
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    let map = retrieved.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(
        map_2.linked_maps.get(&registration_id).map(EntityMapId::as_str),
        some(eq(format!("urn:ngsi-ld:EntityMap:{namespace}-linked").as_str()))
    );
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let headed = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .retrieve(&vehicle_id)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(headed.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map)));
    let forwarded_request = SourceRequest::matching(Method::GET, "/s1/.*").header(HeaderCondition::Is(
        HeaderName::from_static("ngsild-entitymap"),
        format!("urn:ngsi-ld:EntityMap:{namespace}-linked"),
    ));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
