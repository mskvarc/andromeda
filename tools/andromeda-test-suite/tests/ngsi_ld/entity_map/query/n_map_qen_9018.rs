//! A forwarded entity map creation names only what its registration covers.

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
    http::{headers::Accept, operation::EntityMapLifetime},
    model::NgsiLdMediaType,
    query::{EntitySplitting, q, type_selection},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMap, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};
use wiremock::Request;

/// The type and the Attributes one forwarded creation names.
fn selection(request: &Request) -> (String, String) {
    (query_values(request, "type").join(","), query_values(request, "attrs").join(","))
}

/// Pins: over an inclusive `createEntityMapQueryEntity` registration of source a with a block of `Vehicle` Entities
/// and the Attribute speed and a block of `Building` Entities and the Attribute fuel, and one of source b with a block
/// of `Vehicle` Entities and speed, Create `EntityMap` for Query Entities with type `Vehicle|Building`, attrs speed,
/// fuel and temperature, a q, `entityMapLifetime` PT1H and `splitEntities` false asks a once for type `Vehicle` with
/// attrs speed and once for type `Building` with attrs fuel, and b once for type `Vehicle` with attrs speed, each with
/// the q and the lifetime and without a limit, and answers 201 with the Entity of each source's map attributed to its
/// registration and `linkedMaps` naming the map of b and none of a
/// Clause: 4.3.6.1, 5.14.4.3, 5.14.4.4, Table 5.2.39-2
/// Recorded: Table 5.2.39-2 links one map to a registration, and each block of a creates a map of its own, so a is
/// linked to neither
#[gtest]
#[tokio::test]
async fn n_map_qen_9018_a_forwarded_entity_map_creation_names_only_what_its_registration_covers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = namespace.entity_type("Building")?;
    let a_vehicle_id = namespace.urn("Vehicle", "a")?;
    let b_vehicle_id = namespace.urn("Vehicle", "b")?;
    let a_registration_id = namespace.registration_id("a")?;
    let b_registration_id = namespace.registration_id("b")?;
    let a_map_id = format!("urn:ngsi-ld:EntityMap:{namespace}-a").parse::<EntityMapId>()?;
    let b_map_id = format!("urn:ngsi-ld:EntityMap:{namespace}-b").parse::<EntityMapId>()?;
    for (prefix, map_id, entity_id) in [("a", &a_map_id, &a_vehicle_id), ("b", &b_map_id, &b_vehicle_id)] {
        let mut map = EntityMap::new(map_id.clone(), "2099-01-01T00:00:00Z".parse()?);
        map.entity_map.insert(entity_id.clone(), vec![EntitySource::Local]);
        source
            .answer(
                Method::GET,
                &format!("/{prefix}/ngsi-ld/v1/entityMaps"),
                source_answer::entity_map(StatusCode::CREATED, &map),
            )
            .await?;
    }
    let vehicle_block = RegistrationInfo::builder()
        .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
        .property_names(["speed"])
        .build()?;
    let building_block = RegistrationInfo::builder()
        .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
        .property_names(["fuel"])
        .build()?;
    let a_registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(a_registration_id.clone())
        .information([vehicle_block.clone(), building_block])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    let b_registration = CSourceRegistration::builder(source.endpoint("b")?)
        .id(b_registration_id.clone())
        .information([vehicle_block])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntityMapQueryEntity])
        .build()?;
    broker.registrations().register(&a_registration).send().await?;
    broker.registrations().register(&b_registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registrations_cleanup = Cleanup::of(&broker).registration(&a_registration_id).registration(&b_registration_id);
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(type_selection::entity_type(&vehicle_type).or(&building_type))
        .attrs(vec!["speed".parse()?, "fuel".parse()?, "temperature".parse()?])
        .q(q::attr("speed").gt(10))
        .lifetime("PT1H".parse::<EntityMapLifetime>()?)
        .split_entities(EntitySplitting::Whole)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = created.body();
    expect_that!(
        map.entity_map.get(&a_vehicle_id),
        some(elements_are![eq(&EntitySource::Registration(a_registration_id.clone()))])
    );
    expect_that!(
        map.entity_map.get(&b_vehicle_id),
        some(elements_are![eq(&EntitySource::Registration(b_registration_id.clone()))])
    );
    expect_that!(map.linked_maps.get(&a_registration_id), none());
    expect_that!(map.linked_maps.get(&b_registration_id), some(eq(&b_map_id)));
    let asked_a = source.received_on(Method::GET, "/a/ngsi-ld/v1/entityMaps").await?;
    let asked_b = source.received_on(Method::GET, "/b/ngsi-ld/v1/entityMaps").await?;
    expect_that!(
        asked_a.iter().map(selection).collect::<Vec<_>>(),
        unordered_elements_are![
            eq(&(vehicle_type.to_string(), "speed".to_owned())),
            eq(&(building_type.to_string(), "fuel".to_owned()))
        ]
    );
    expect_that!(
        asked_b.iter().map(selection).collect::<Vec<_>>(),
        elements_are![eq(&(vehicle_type.to_string(), "speed".to_owned()))]
    );
    for request in asked_a.iter().chain(&asked_b) {
        expect_that!(query_values(request, "q"), elements_are![eq("speed>10")]);
        expect_that!(query_values(request, "entityMapLifetime"), elements_are![eq("PT1H")]);
        expect_that!(query_values(request, "limit"), is_empty());
    }
    Ok(())
}
