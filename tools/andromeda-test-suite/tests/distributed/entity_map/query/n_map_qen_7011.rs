//! With split entities the filters are not applied locally and not forwarded.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    received_status::ExpectedStatus,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{EntitySplitting, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Entities with `splitEntities` true maps the local Entities by type and
/// `idPattern`, keeping one that fails q and leaving out one outside the `idPattern`, and forwards the request to
/// the registration advertising `createEntityMapQueryEntity` without q and attrs
/// Clause: 5.14.4.4, 5.7.2.4, 4.20
#[gtest]
#[tokio::test]
async fn n_map_qen_7011_with_split_entities_the_filters_are_not_applied_locally_and_not_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_50_id = namespace.urn("Vehicle", 50)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let entities = [
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 20)
            .build()?,
        Entity::builder()
            .id(vehicle_3_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 5)
            .build()?,
        Entity::builder()
            .id(vehicle_50_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 20)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let mut source_map = EntityMap::new(
        format!("urn:ngsi-ld:EntityMap:{namespace}-remote").parse::<EntityMapId>()?,
        "2099-01-01T00:00:00Z".parse()?,
    );
    source_map.entity_map.insert(vehicle_2_id.clone(), vec![EntitySource::Local]);
    let source_map = source_map;
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/entityMaps",
            source_answer::entity_map(StatusCode::CREATED, &source_map),
        )
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
        .id_pattern(namespace.id_pattern_ending("Vehicle", "[0-9]$")?)
        .q(q::attr("speed").gt(10))
        .attrs(vec!["speed".parse()?])
        .split_entities(EntitySplitting::Split)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(3)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map.entity_map.get(&vehicle_3_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map.entity_map.get(&vehicle_50_id), none());
    expect_that!(
        map.entity_map.get(&vehicle_2_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(
        map.linked_maps.get(&registration_id).map(EntityMapId::as_str),
        some(eq(format!("urn:ngsi-ld:EntityMap:{namespace}-remote").as_str()))
    );
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entityMaps").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "type"), nth(0, eq(format!("Vehicle_{namespace}").as_str())));
    expect_that!(query_values(forwarded_one, "q"), is_empty());
    expect_that!(query_values(forwarded_one, "attrs"), is_empty());
    Ok(())
}
