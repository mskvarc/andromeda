//! The last page reached by next offers only prev.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Query Entities with limit 2 over four members, followed by its next pointer, answers the last two members
/// with a prev pointer and no first, last or next pointer
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_qry_pag_0036_the_last_page_reached_by_next_offers_only_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0001_id = namespace.urn("Building", "0001")?;
    let building_0002_id = namespace.urn("Building", "0002")?;
    let building_0003_id = namespace.urn("Building", "0003")?;
    let building_0000 = Entity::builder()
        .id(building_0000_id.clone())
        .entity_type(building_type.clone())
        .property("name", "b0")
        .build()?;
    broker.entities().create(&building_0000).send().await?.done()?;
    let building_0001 = Entity::builder()
        .id(building_0001_id.clone())
        .entity_type(building_type.clone())
        .property("name", "b1")
        .build()?;
    broker.entities().create(&building_0001).send().await?.done()?;
    let building_0002 = Entity::builder()
        .id(building_0002_id.clone())
        .entity_type(building_type.clone())
        .property("name", "b2")
        .build()?;
    broker.entities().create(&building_0002).send().await?.done()?;
    let building_0003 = Entity::builder()
        .id(building_0003_id.clone())
        .entity_type(building_type.clone())
        .property("name", "b3")
        .build()?;
    broker.entities().create(&building_0003).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&building_0001_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.prev(), some(anything()));
    expect_that!(page_2.first(), none());
    expect_that!(page_2.last(), none());
    expect_that!(page_2.next(), none());
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&building_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_0003_id)));
    Ok(())
}
