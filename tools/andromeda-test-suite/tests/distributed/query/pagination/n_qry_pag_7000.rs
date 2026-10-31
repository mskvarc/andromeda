//! A split entity that stopped matching after the merge is shed from the reused map.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    cleanup::Cleanup,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    received_status::ExpectedStatus,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::{Entity, EntityFragment, NgsiLdMediaType},
    query::{EntitySplitting, q},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, EntitySource, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities with `splitEntities` true and q speed>50 naming an `EntityMap` built over two Entities
/// broker-a holds as speed and broker-b as fuel answers only the Entity whose speed is still above 50 after one is
/// lowered to 10, and the `EntityMap` then no longer lists the Entity that stopped matching
/// Clause: 5.5.9.3, 5.5.14, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_qry_pag_7000_a_split_entity_that_stopped_matching_after_the_merge_is_shed_from_the_reused_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_stays_id = namespace.urn("Vehicle", "stays")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_sheds_id = namespace.urn("Vehicle", "sheds")?;
    let broker_b = broker::client(BrokerRole::B)?;
    let registration_id = namespace.registration_id(1)?;
    let entities = [
        Entity::builder()
            .id(vehicle_stays_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 60)
            .build()?,
        Entity::builder()
            .id(vehicle_sheds_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", 70)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let entities_2 = [
        Entity::builder()
            .id(vehicle_stays_id.clone())
            .entity_type(vehicle_type.clone())
            .property("fuel", 30)
            .build()?,
        Entity::builder()
            .id(vehicle_sheds_id.clone())
            .entity_type(vehicle_type.clone())
            .property("fuel", 40)
            .build()?,
    ];
    broker_b.entities().batch_create(&entities_2).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["fuel"])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .split_entities(EntitySplitting::Split)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.items(), len(eq(2)));
    let fragment = EntityFragment::builder().property("speed", 10).build()?;
    broker.entities().update_attributes(&vehicle_sheds_id, &fragment).send().await?.done()?;
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .split_entities(EntitySplitting::Split)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    let page_2 = headed.outcome;
    expect_that!(headed.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(0, all!(has_id(&vehicle_stays_id), has_attribute("speed", any_attribute().value(60))))
    );
    let map_id = map.id().or_fail()?;
    let map_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&map_id)
        .send()
        .await?;
    expect_that!(map_2.entity_map.get(&vehicle_stays_id), some(contains(eq(&EntitySource::Local))));
    expect_that!(map_2.entity_map.get(&vehicle_sheds_id), none());
    Ok(())
}
