//! An order by id keeps offset pointers.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Upserted,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, ordering::OrderBy, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with `orderBy` id and limit 2 over three entities answers a next
/// pointer carrying an offset and no cursor
/// Clause: 5.7.4.3, 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 6.3.10 says the pointers of the transparent option "should include" limit and offset; Andromeda keeps
/// offset pointers when the request names an offset or an order
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0022_an_order_by_id_keeps_offset_pointers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sensor_0000_id = namespace.urn("Sensor", "0000")?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let sensor_0001_id = namespace.urn("Sensor", "0001")?;
    let sensor_0002_id = namespace.urn("Sensor", "0002")?;
    let sensor_0000_evolution = TemporalEntity::builder()
        .id(sensor_0000_id)
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0000_evolution).send().await?.created()?;
    let sensor_0001_evolution = TemporalEntity::builder()
        .id(sensor_0001_id)
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0001_evolution).send().await?.created()?;
    let sensor_0002_evolution = TemporalEntity::builder()
        .id(sensor_0002_id)
        .entity_type(sensor_type.clone())
        .property("reading", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&sensor_0002_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::after("1970-01-01T00:00:00Z".parse()?))
        .entity_type(&sensor_type)
        .locality(Locality::Local)
        .ordering("id".parse::<OrderBy>()?)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.next(), some(anything()));
    expect_that!(links(&page), contains(carries_parameter("offset")));
    expect_that!(links(&page), each(not(carries_parameter("cursor"))));
    Ok(())
}
