//! A purge under an entity map header deletes only the entities the map lists and names the map.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::Entity,
    query::Locality,
};

/// Pins: Purge Entities by type and two ids under the `NGSILD-EntityMap` header of a map listing one of them
/// deletes that entity only and answers 204 with the same header
/// Clause: 6.4.3.3, 5.6.21.4, 6.3.15
#[gtest]
#[tokio::test]
async fn n_ent_prg_7200_a_purge_under_an_entity_map_header_deletes_only_the_entities_the_map_lists_and_names_the_map() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 6)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let map = page.entity_map().valid().or_fail()?;
    let received = broker
        .with_responses()
        .entities()
        .purge()
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone(), vehicle_2_id.clone()])
        .entity_map(map)
        .send()
        .await?;
    received.outcome.done()?;
    expect_that!(received.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map)));
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&vehicle_2_id)));
    Ok(())
}
