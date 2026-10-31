//! The pages of a map over two peers are disjoint and cover every entity.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, PeerRole},
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, ResultsCount},
    },
    model::{Entity, EntityId},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type with `entityMap` true, limit 2 and count true, over inclusive
/// registrations of brokers B and C holding three Entities each, answers 201 with two Entities and a count of 6,
/// and the same query naming the map at offset 2 and 4 answers two further Entities each, none repeated
/// Clause: 5.7.2.4, 5.5.14, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_map_qen_8000_the_pages_of_a_map_over_two_peers_are_disjoint_and_cover_every_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_type = namespace.entity_type("Building")?;
    let broker_c = broker::client(BrokerRole::C)?;
    let registration_b_id = namespace.registration_id("b")?;
    let registration_c_id = namespace.registration_id("c")?;
    let entities_b = [
        Entity::builder()
            .id(namespace.urn("Building", "b0")?)
            .entity_type(building_type.clone())
            .property("speed", 0)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "b1")?)
            .entity_type(building_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "b2")?)
            .entity_type(building_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    broker_b.entities().batch_create(&entities_b).send().await?.done()?;
    let entities_c = [
        Entity::builder()
            .id(namespace.urn("Building", "c0")?)
            .entity_type(building_type.clone())
            .property("speed", 0)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "c1")?)
            .entity_type(building_type.clone())
            .property("speed", 1)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Building", "c2")?)
            .entity_type(building_type.clone())
            .property("speed", 2)
            .build()?,
    ];
    broker_c.entities().batch_create(&entities_c).send().await?.done()?;
    let registration_b = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_b_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_b).send().await?;
    let registration_c = CSourceRegistration::builder(broker::peer(PeerRole::C)?)
        .id(registration_c_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration_c).send().await?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .entity_map(&entity_map_request)
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.total(), some(eq(6)));
    expect_that!(page.items(), len(eq(2)));
    let map_header = page.entity_map().valid().or_fail()?;
    let first_page_ids: Vec<EntityId> = page.items().iter().map(|entity| entity.id.clone()).collect();
    let entity_map_request_2 = EntityMapRequest::returned().reusing(map_header.clone());
    let page_2 = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .entity_map(&entity_map_request_2)
        .limit(2)
        .count(ResultsCount::Include)
        .offset(2)
        .send()
        .await?
        .expecting(StatusCode::OK)?;
    expect_that!(page_2.total(), some(eq(6)));
    expect_that!(page_2.items(), len(eq(2)));
    for id in &first_page_ids {
        expect_that!(page_2.items(), not(contains(has_id(id))));
    }
    let second_page_ids: Vec<EntityId> = page_2.items().iter().map(|entity| entity.id.clone()).collect();
    let entity_map_request_3 = EntityMapRequest::returned().reusing(map_header.clone());
    let page_3 = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .entity_map(&entity_map_request_3)
        .limit(2)
        .count(ResultsCount::Include)
        .offset(4)
        .send()
        .await?
        .expecting(StatusCode::OK)?;
    expect_that!(page_3.total(), some(eq(6)));
    expect_that!(page_3.items(), len(eq(2)));
    for id in first_page_ids.iter().chain(&second_page_ids) {
        expect_that!(page_3.items(), not(contains(has_id(id))));
    }
    Ok(())
}
