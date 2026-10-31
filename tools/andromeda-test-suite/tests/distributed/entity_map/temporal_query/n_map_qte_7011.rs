//! With split entities the filters are not applied locally and not forwarded.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    received_status::ExpectedStatus,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with `splitEntities` true maps the local
/// Entities by type and `idPattern` within the interval, leaving out one outside the `idPattern`, and forwards the
/// request to the registration advertising `createEntityMapQueryTemporal` with the type, the temporal query and no
/// q
/// Clause: 5.14.5.4, 5.7.4.4, 4.20
#[gtest]
#[tokio::test]
async fn n_map_qte_7011_with_split_entities_the_filters_are_not_applied_locally_and_not_forwarded() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_50_id = namespace.urn("Vehicle", 50)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_50_evolution = TemporalEntity::builder()
        .id(vehicle_50_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_50_evolution).send().await?.created()?;
    let mut source_map = EntityMap::new(
        format!("urn:ngsi-ld:EntityMap:{namespace}-remote").parse::<EntityMapId>()?,
        "2099-01-01T00:00:00Z".parse()?,
    );
    source_map.entity_map.insert(vehicle_2_id.clone(), vec![EntitySource::Local]);
    let source_map = source_map;
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entityMaps",
            source_answer::entity_map(StatusCode::CREATED, &source_map),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .id_pattern(namespace.id_pattern_ending("Vehicle", "[0-9]$")?)
        .q(q::attr("speed").gt(10))
        .split_entities(EntitySplitting::Split)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map.entity_map.get(&vehicle_50_id), none());
    expect_that!(
        map.entity_map.get(&vehicle_2_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(
        map.linked_maps.get(&registration_id).map(EntityMapId::as_str),
        some(eq(format!("urn:ngsi-ld:EntityMap:{namespace}-remote").as_str()))
    );
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "type"), nth(0, eq(format!("Vehicle_{namespace}").as_str())));
    expect_that!(query_values(forwarded_one, "timerel"), nth(0, eq("before")));
    expect_that!(query_values(forwarded_one, "q"), is_empty());
    Ok(())
}
