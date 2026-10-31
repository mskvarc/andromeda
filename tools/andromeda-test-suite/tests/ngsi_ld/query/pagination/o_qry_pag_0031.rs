//! An explicit order by without local keeps offset pointers.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::ordering::OrderBy,
};

/// Pins: Query Entities with `orderBy` name and limit 2 over three entities without local answers a next pointer
/// carrying an offset and no cursor
/// Clause: 5.5.9.2, 6.3.10, 4.23
/// Recorded: 6.3.10 says the pointers of the transparent option "should include" limit and offset; Andromeda keeps
/// offset pointers when the request names an offset or an order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0031_an_explicit_order_by_without_local_keeps_offset_pointers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0001_id = namespace.urn("Building", "0001")?;
    let building_0002_id = namespace.urn("Building", "0002")?;
    let building_0000 = Entity::builder()
        .id(building_0000_id)
        .entity_type(building_type.clone())
        .property("name", "b0")
        .build()?;
    broker.entities().create(&building_0000).send().await?.done()?;
    let building_0001 = Entity::builder()
        .id(building_0001_id)
        .entity_type(building_type.clone())
        .property("name", "b1")
        .build()?;
    broker.entities().create(&building_0001).send().await?.done()?;
    let building_0002 = Entity::builder()
        .id(building_0002_id)
        .entity_type(building_type.clone())
        .property("name", "b2")
        .build()?;
    broker.entities().create(&building_0002).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .ordering("name".parse::<OrderBy>()?)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.next(), some(anything()));
    expect_that!(links(&page), contains(carries_parameter("offset")));
    expect_that!(links(&page), each(not(carries_parameter("cursor"))));
    Ok(())
}
