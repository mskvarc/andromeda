//! Pages through an entity map cover every mapped entity once.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, EntityId, NgsiLdMediaType, Property},
};
use std::collections::HashSet;

/// Pins: Query Entities under an `NGSILD-EntityMap` of ten Entities, paged by limit 3 at offsets 0, 3, 6 and 9,
/// answers pages of 3, 3, 3 and 1 Entities, each naming the map and no Entity twice
/// Clause: 6.4.3.2, 5.5.9, 5.14.4
#[gtest]
#[tokio::test]
async fn n_qry_pag_0004_pages_through_an_entity_map_cover_every_mapped_entity_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let mut entities = Vec::new();
    for n in 0..10 {
        entities.push(
            Entity::builder()
                .id(namespace.urn("Vehicle", n)?)
                .entity_type(vehicle_type.clone())
                .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let created = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    expect_that!(&created.body().entity_map, len(eq(10)));
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let mut seen: HashSet<EntityId> = HashSet::new();
    for (offset, expected) in [(0, 3), (3, 3), (6, 3), (9, 1)] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&vehicle_type)
            .limit(3)
            .offset(offset)
            .entity_map(&entity_map_request)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(expected)));
        expect_that!(page.entity_map().valid(), some(anything()));
        for item in page.items() {
            expect_that!(seen.insert(item.id.clone()), eq(true));
        }
    }
    expect_that!(seen, len(eq(10)));
    Ok(())
}
