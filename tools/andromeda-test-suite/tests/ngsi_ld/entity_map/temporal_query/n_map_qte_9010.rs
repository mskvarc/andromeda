//! A forwarded temporal entity map creation names only what its registration covers.

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
    model::{NgsiLdMediaType, evolution::TemporalAttributeName},
    query::{EntitySplitting, temporal_query, type_selection},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: over an inclusive `createEntityMapQueryTemporal` registration of `Vehicle` Entities and the Attribute speed,
/// Create `EntityMap` for Query Temporal Evolution of Entities before 2022-01-01T00:00:00Z with `lastN` 2, type
/// `Vehicle|Building`, attrs speed and fuel and `splitEntities` false asks the source once, for type `Vehicle` with
/// attrs speed, `timerel` before, `timeAt` 2022-01-01T00:00:00Z and `lastN` 2 and without a limit, and answers 201
/// with the Entity of the source's map attributed to the registration and `linkedMaps` naming the source's map
/// Clause: 4.3.6.1, 5.14.5.3, 5.14.5.4
#[gtest]
#[tokio::test]
async fn n_map_qte_9010_a_forwarded_temporal_entity_map_creation_names_only_what_its_registration_covers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let map_id = format!("urn:ngsi-ld:EntityMap:{namespace}-remote").parse::<EntityMapId>()?;
    let mut map = EntityMap::new(map_id.clone(), "2099-01-01T00:00:00Z".parse()?);
    map.entity_map.insert(vehicle_id.clone(), vec![EntitySource::Local]);
    let map = map;
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entityMaps",
            source_answer::entity_map(StatusCode::CREATED, &map),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
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
        .create_temporal(temporal_query::before("2022-01-01T00:00:00Z".parse()?).last_n(2))
        .entity_type(type_selection::entity_type(&vehicle_type).or(&building_type))
        .attrs(vec![
            TemporalAttributeName::Attribute("speed".parse()?),
            TemporalAttributeName::Attribute("fuel".parse()?),
        ])
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let created_map = created.body();
    expect_that!(
        created_map.entity_map.get(&vehicle_id),
        some(elements_are![eq(&EntitySource::Registration(registration_id.clone()))])
    );
    expect_that!(created_map.linked_maps.get(&registration_id), some(eq(&map_id)));
    let asked = source.received_on(Method::GET, "/a/ngsi-ld/v1/temporal/entityMaps").await?;
    expect_that!(asked, len(eq(1)));
    let asked = asked.first().or_fail()?;
    expect_that!(query_values(asked, "type"), elements_are![eq(vehicle_type.as_str())]);
    expect_that!(query_values(asked, "attrs"), elements_are![eq("speed")]);
    expect_that!(query_values(asked, "timerel"), elements_are![eq("before")]);
    expect_that!(query_values(asked, "timeAt"), elements_are![eq("2022-01-01T00:00:00Z")]);
    expect_that!(query_values(asked, "lastN"), elements_are![eq("2")]);
    expect_that!(query_values(asked, "limit"), is_empty());
    Ok(())
}
