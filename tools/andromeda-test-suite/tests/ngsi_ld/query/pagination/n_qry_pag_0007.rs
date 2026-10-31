//! A count under an entity map counts every mapped entity.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{EntityMapRequest, ResultsCount},
    },
    model::{Entity, Input, NgsiLdMediaType, Property},
};

/// Pins: Query Entities under an `NGSILD-EntityMap` of five Entities with count=true and limit 2 answers two
/// Entities and NGSILD-Results-Count 5
/// Clause: 6.4.3.2, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_qry_pag_0007_a_count_under_an_entity_map_counts_every_mapped_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let entities = (0..=4)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", n)?)
                .entity_type(vehicle_type.clone())
                .property("speed", Property::builder(50).observed_at("2021-01-01T00:00:00Z".parse()?))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
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
    let map_header = created.entity_map().valid().or_fail()?;
    let entity_map_request = EntityMapRequest::reuse(map_header.clone());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .count(ResultsCount::Include)
        .limit(2)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.total(), some(eq(5)));
    Ok(())
}
