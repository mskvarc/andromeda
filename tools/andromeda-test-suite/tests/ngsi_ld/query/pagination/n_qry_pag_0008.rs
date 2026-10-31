//! A full first page offers next with the requested media type.

use andromeda_test_suite::{broker, link_matcher::link_value, namespace::Namespace, outcome::Done, wire::header_values};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, Link, RelationType},
    model::{Entity, NgsiLdMediaType},
};

/// Pins: Query Entities as application/ld+json with limit 2 and offset 0 over three entities answers a next pointer
/// of type application/ld+json and no prev pointer
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_qry_pag_0008_a_full_first_page_offers_next_with_the_requested_media_type() -> Result<()> {
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
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .offset(0)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed.head.typed::<Link>().valid(),
        some(link_value(RelationType::Next).with_media_type("application/ld+json"))
    );
    expect_that!(page.next(), some(anything()));
    expect_that!(page.prev(), none());
    Ok(())
}
