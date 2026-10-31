//! A query over a map answers only the mapped entities across pages.

use andromeda_test_suite::{broker, header_matcher::present, identity_matcher::has_id, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::{Accept, NgsiLdEntityMap},
        operation::EntityMapRequest,
    },
    model::Entity,
};

/// Pins: Query Entities by type naming an `EntityMap` built over two Entities answers 200 with the same
/// `NGSILD-EntityMap` header and only those two, one per page with limit 1, and not a third Entity created
/// afterwards
/// Clause: 4.3.6.7, 5.5.14, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_map_qen_6401_a_query_over_a_map_answers_only_the_mapped_entities_across_pages() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let vehicle_2 = Entity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 20)
        .build()?;
    broker.entities().create(&vehicle_2).send().await?.done()?;
    let entity_map_request = EntityMapRequest::returned();
    let page = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(page.entity_map(), present());
    let vehicle_3 = Entity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 30)
        .build()?;
    broker.entities().create(&vehicle_3).send().await?.done()?;
    let map = page.entity_map().valid().or_fail()?;
    let entity_map_request_2 = EntityMapRequest::reuse(map.clone());
    let headed = broker
        .with_heads()
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .limit(1)
        .entity_map(&entity_map_request_2)
        .send()
        .await?;
    let page_2 = headed.outcome;
    expect_that!(headed.head.typed::<NgsiLdEntityMap>().valid(), some(eq(map)));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items().first().map(|item| item.id.to_string()), some(contains_regex("-[12]$")));
    let entity_map_request_3 = EntityMapRequest::reuse(map.clone());
    let page_3 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .entity_map(&entity_map_request_3)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&vehicle_id)));
    expect_that!(page_3.items(), contains(has_id(&vehicle_2_id)));
    expect_that!(page_3.items(), not(contains(has_id(&vehicle_3_id))));
    Ok(())
}
