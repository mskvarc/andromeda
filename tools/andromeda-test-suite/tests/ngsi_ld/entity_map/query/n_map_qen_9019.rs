//! A query creating a map links no map of a registration asked once per block.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    forwarded_query::query_values,
    header_condition::HeaderCondition,
    header_matcher::present,
    namespace::Namespace,
    received_status::ExpectedStatus,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, NgsiLdMediaType},
    query::type_selection,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntityMapId, EntitySource, RegistrationInfo, RegistrationMode},
};
use wiremock::Request;

/// The forwarded requests to source a that carried `NGSILD-EntityMap` naming `map`.
async fn told(source: &ContextSource, map: &str) -> Result<Vec<Request>> {
    let request = SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities")
        .header(HeaderCondition::Contains(HeaderName::from_static("ngsild-entitymap"), map.to_owned()));
    Ok(source.received_matching(&request).await?)
}

/// Pins: over an inclusive `queryEntity` registration of source a with a block of `Vehicle` Entities and the
/// Attribute speed and a block of `Building` Entities and the Attribute fuel, whose source answers the `Vehicle` read
/// with its Entity under one `EntityMap` and the `Building` read with its Entity under another, Query Entities with
/// type `Vehicle|Building`, attrs speed and fuel and `entityMap` true answers 201, and the `EntityMap` it names
/// attributes both Entities to the registration and links it to neither map; a second Query Entities under that map
/// leaves the registration linked to neither, and every forwarded request of both queries names the broker's map and
/// none names a map of source a
/// Clause: 4.3.6.3, 5.5.14, 5.7.2.4, Table 5.2.39-2
/// Recorded: Table 5.2.39-2 links one map to a registration and clause 5.5.14 pins a map to the request that created
/// it, so a registration whose blocks are read apart is linked to neither block's map, as a forwarded `EntityMap`
/// creation is
#[gtest]
#[tokio::test]
async fn n_map_qen_9019_a_query_creating_a_map_links_no_map_of_a_registration_asked_once_per_block() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let building_type = namespace.entity_type("Building")?;
    let vehicle_id = namespace.urn("Vehicle", "a")?;
    let building_id = namespace.urn("Building", "a")?;
    let registration_id = namespace.registration_id("a")?;
    let vehicles_map = format!("urn:ngsi-ld:EntityMap:{namespace}-vehicles").parse::<EntityMapId>()?;
    let buildings_map = format!("urn:ngsi-ld:EntityMap:{namespace}-buildings").parse::<EntityMapId>()?;
    let vehicle = Entity::output_builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 25)
        .build()?;
    let building = Entity::output_builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("fuel", 40)
        .build()?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("type", &vehicle_type),
            source_answer::entities(&[vehicle]).insert_header("ngsild-entitymap", vehicles_map.as_str()),
        )
        .await?;
    source
        .answer_when(
            SourceRequest::new(Method::GET, "/a/ngsi-ld/v1/entities").query("type", &building_type),
            source_answer::entities(&[building]).insert_header("ngsild-entitymap", buildings_map.as_str()),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([
            RegistrationInfo::builder()
                .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
                .property_names(["speed"])
                .build()?,
            RegistrationInfo::builder()
                .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
                .property_names(["fuel"])
                .build()?,
        ])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&vehicle_type).or(&building_type))
        .attrs(vec!["speed".parse()?, "fuel".parse()?])
        .entity_map(&EntityMapRequest::returned())
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let map = page.entity_map().valid().or_fail()?;
    let map_id = map.id().or_fail()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let minted = json.entity_maps().retrieve(&map_id).send().await?;
    expect_that!(
        minted.entity_map.get(&vehicle_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(
        minted.entity_map.get(&building_id),
        some(contains(eq(&EntitySource::Registration(registration_id.clone()))))
    );
    expect_that!(minted.linked_maps.get(&registration_id), none());
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&vehicle_type).or(&building_type))
        .attrs(vec!["speed".parse()?, "fuel".parse()?])
        .entity_map(&EntityMapRequest::reuse(map.clone()))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    let reused = json.entity_maps().retrieve(&map_id).send().await?;
    expect_that!(reused.linked_maps.get(&registration_id), none());
    expect_that!(told(&source, vehicles_map.as_str()).await?, is_empty());
    expect_that!(told(&source, buildings_map.as_str()).await?, is_empty());
    // Both blocks are read for each query, and each read names the map the broker minted.
    let told_broker_map = told(&source, map.as_iri_ref().as_str()).await?;
    expect_that!(
        told_broker_map
            .iter()
            .map(|request| query_values(request, "type").join(","))
            .collect::<Vec<_>>(),
        unordered_elements_are![
            eq(vehicle_type.as_str()),
            eq(vehicle_type.as_str()),
            eq(building_type.as_str()),
            eq(building_type.as_str())
        ]
    );
    json.entity_maps().delete(&map_id).send().await?;
    Ok(())
}
