//! A forwarded entity map creation carries the Linked Entity retrieval of its q.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{EntitySplitting, Join, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: over an inclusive `createEntityMapQueryEntity` registration of `Vehicle` Entities, Create `EntityMap` for
/// Query Entities by type with the q `isParkedIn{category}=="commercial"`, `join` flat, `joinLevel` 2 and
/// `splitEntities` false asks the source once with that q, `join` flat and `joinLevel` 2, and answers 201 with the
/// Entity of the source's map attributed to the registration
/// Clause: 5.14.4.3, 5.14.4.4, 4.5.23
/// Recorded: clause 5.14.4.4 makes a source refuse a q over Linked Entity Attributes without `join` as too deep a
/// query, and removes only the filters before forwarding, so the Linked Entity retrieval travels with the q
#[gtest]
#[tokio::test]
async fn n_map_qen_9020_a_forwarded_entity_map_creation_carries_the_linked_entity_retrieval_of_its_q() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let mut map = EntityMap::new(
        format!("urn:ngsi-ld:EntityMap:{namespace}-remote").parse::<EntityMapId>()?,
        "2099-01-01T00:00:00Z".parse()?,
    );
    map.entity_map.insert(vehicle_id.clone(), vec![EntitySource::Local]);
    let map = map;
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entityMaps", source_answer::entity_map(StatusCode::CREATED, &map))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(
        created.body().entity_map.get(&vehicle_id),
        some(elements_are![eq(&EntitySource::Registration(registration_id.clone()))])
    );
    let asked = source.received_on(Method::GET, "/a/ngsi-ld/v1/entityMaps").await?;
    expect_that!(asked, len(eq(1)));
    let asked = asked.first().or_fail()?;
    expect_that!(query_values(asked, "q"), elements_are![eq("isParkedIn{category}==\"commercial\"")]);
    expect_that!(query_values(asked, "join"), elements_are![eq("flat")]);
    expect_that!(query_values(asked, "joinLevel"), elements_are![eq("2")]);
    Ok(())
}
