//! A single page holding every entity offers no pointers.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities with limit 10 and offset 0 over two entities answers both entities and no next or prev
/// pointer
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_qry_pag_0011_a_single_page_holding_every_entity_offers_no_pointers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0001_id = namespace.urn("Building", "0001")?;
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(10)
        .offset(0)
        .send()
        .await?;
    expect_that!(page.next(), none());
    expect_that!(page.prev(), none());
    expect_that!(page.items(), len(eq(2)));
    Ok(())
}
