//! The first page of a cursor walk offers opaque cursor pointers.

use andromeda_test_suite::{
    broker,
    cursor_walk,
    namespace::Namespace,
    outcome::Upserted,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryTemporalEvolution},
    model::{Lossless, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{Locality, builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with limit 2 and an empty cursor over three members answers a first
/// page whose Link offers next and last pointers carrying a cursor and no offset, and no first or prev
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_tmp_qry_0018_the_first_page_of_a_cursor_walk_offers_opaque_cursor_pointers() -> Result<()> {
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
    let window = temporal_query::after("1970-01-01T00:00:00Z".parse()?).build()?;
    let selector = SelectorTypeBuilder::from(&sensor_type).build()?;
    let query = QueryTemporalEvolution::builder()
        .temporal_q(&window)
        .entity_type(&selector)
        .locality(Locality::Local)
        .limit(2)
        .format(Lossless::Normalized)
        .build();
    let page = cursor_walk::first_page(&broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)), &query)
        .await?
        .page;
    expect_that!(page.next(), some(anything()));
    expect_that!(page.last(), some(anything()));
    expect_that!(page.first(), none());
    expect_that!(page.prev(), none());
    expect_that!(links(&page), contains(carries_parameter("cursor")));
    expect_that!(links(&page), each(not(carries_parameter("offset"))));
    Ok(())
}
