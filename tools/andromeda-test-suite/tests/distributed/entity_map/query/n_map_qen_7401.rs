//! A named map fixes the sources consulted and an unknown map is made anew.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    context_source::ContextSource,
    entity_matcher::has_attribute,
    namespace::Namespace,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Entity with `entityMap` true answers 200 with an `NGSILD-EntityMap` header, the same retrieval
/// naming that map answers 200 with the same header and never reaches a source registered after the map was made,
/// and a retrieval naming an unknown map reaches that source
/// Clause: 5.7.1.4, 5.5.14, 6.5.3.1
#[gtest]
#[tokio::test]
async fn n_map_qen_7401_a_named_map_fixes_the_sources_consulted_and_an_unknown_map_is_made_anew() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("s1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let source_entity = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    let s1_path = format!("/s1/ngsi-ld/v1/entities/{vehicle_id}");
    source.answer(Method::GET, &s1_path, source_answer::entity(&source_entity)).await?;
    let entity_map_request = EntityMapRequest::returned();
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(retrieved.body(), has_attribute("speed", any_attribute().value(50)));
    let map_header = retrieved.entity_map().valid().or_fail()?;
    let registration_2 = CSourceRegistration::builder(source.endpoint("s2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .operations([DistributedOperation::RetrieveEntity])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let source_entity_2 = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    source
        .answer(
            Method::GET,
            &format!("/s2/ngsi-ld/v1/entities/{vehicle_id}"),
            source_answer::entity(&source_entity_2),
        )
        .await?;
    let entity_map_request_2 = EntityMapRequest::reuse(map_header.clone());
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    expect_that!(retrieved_2.entity_map().valid(), some(eq(map_header)));
    expect_that!(retrieved_2.body(), has_attribute("speed", any_attribute().value(50)));
    let consulted = source.received_on(Method::GET, &s1_path).await?;
    expect_that!(consulted, len(eq(2)));
    let second_source = SourceRequest::any_matching("/s2/.*");
    let reached = source.received_matching(&second_source).await?;
    expect_that!(reached, len(eq(0)));
    let bogus_map = NgsiLdEntityMap::try_new(IriRefBuf::new(namespace.urn_text("EntityMap", "bogus"))?)?;
    let entity_map_request_3 = EntityMapRequest::reuse(bogus_map);
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .entity_map(&entity_map_request_3)
        .send()
        .await?;
    expect_that!(retrieved_3.entity_map().valid(), some(anything()));
    let reached_2 = source.received_matching(&second_source).await?;
    expect_that!(reached_2, len(ge(1)));
    Ok(())
}
