//! A registration supporting temporal map creation is asked and its map is folded in.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_head,
    forwarded_query::query_values,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, ReceivedBy},
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Create `EntityMap` for Query Temporal Evolution of Entities with `splitEntities` false is forwarded as GET
/// `/temporal/entityMaps` with the type, the q and the temporal query to the registration advertising
/// `createEntityMapQueryTemporal` and not to one advertising only `queryTemporal`, and the answered map is folded
/// in: the source's Entity maps to its registration, `linkedMaps` maps the registration to the source's map id, the
/// local Entity failing q is left out, and Retrieve `EntityMap` reads the same content
/// Clause: 5.14.5.4, 4.20, 5.2.39, 6.3.18
#[gtest]
#[tokio::test]
async fn n_map_qte_7010_a_registration_supporting_temporal_map_creation_is_asked_and_its_map_is_folded_in() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(20).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_3_evolution = TemporalEntity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(5).observed_at("2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_3_evolution).send().await?.created()?;
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
    let registration_2 = CSourceRegistration::builder(source.endpoint("b")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(10))
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map_id = &created.body().id;
    expect_that!(created.entity_map().valid().map(|map| map.as_iri_ref().as_str()), some(eq(map_id.as_str())));
    let map = created.body();
    expect_that!(&map.entity_map, len(eq(2)));
    expect_that!(map.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(
        map.entity_map.get(&vehicle_2_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(map.entity_map.get(&vehicle_3_id), none());
    expect_that!(
        map.linked_maps.get(&registration_id).map(EntityMapId::as_str),
        some(eq(format!("urn:ngsi-ld:EntityMap:{namespace}-remote").as_str()))
    );
    expect_that!(map.linked_maps.get(&registration_2_id), none());
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(map_id)
        .send()
        .await?;
    expect_that!(&map_2.entity_map, len(eq(2)));
    expect_that!(map_2.entity_map.get(&vehicle_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(
        map_2.entity_map.get(&vehicle_2_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(
        map_2.linked_maps.get(&registration_id).map(EntityMapId::as_str),
        some(eq(format!("urn:ngsi-ld:EntityMap:{namespace}-remote").as_str()))
    );
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    expect_that!(query_values(forwarded_one, "type"), nth(0, eq(format!("Vehicle_{namespace}").as_str())));
    expect_that!(query_values(forwarded_one, "q"), nth(0, eq("speed>10")));
    expect_that!(query_values(forwarded_one, "timerel"), nth(0, eq("before")));
    expect_that!(query_values(forwarded_one, "timeAt"), nth(0, eq("2022-01-01T00:00:00Z")));
    expect_that!(forwarded_head::via_hosts(forwarded_one), some(contains(eq(&"broker-a".parse::<ReceivedBy>()?))));
    let forwarded_request = SourceRequest::any_matching("/b/.*");
    let forwarded_2 = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded_2, len(eq(0)));
    Ok(())
}
