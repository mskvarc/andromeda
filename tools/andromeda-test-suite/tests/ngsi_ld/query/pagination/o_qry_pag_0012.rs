//! The first page of a cursor walk offers opaque cursor pointers.

use andromeda_test_suite::{
    broker,
    cursor_walk,
    namespace::Namespace,
    outcome::Done,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryEntities},
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{Locality, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities with limit 2 and an empty cursor over three members answers a first page whose Link offers
/// next and last pointers carrying a cursor and no offset, and no first or prev
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_qry_pag_0012_the_first_page_of_a_cursor_walk_offers_opaque_cursor_pointers() -> Result<()> {
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
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let query = QueryEntities::builder()
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
