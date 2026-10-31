//! A page reached backward holding the first members offers no prev.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Query Entities with limit 2 over four members, followed by next and then by the second page's prev
/// pointer, answers the first two members with no prev pointer, there being no previous members
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_qry_pag_0021_a_page_reached_backward_holding_the_first_members_offers_no_prev() -> Result<()> {
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
    expect_that!(page.prev(), none());
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&building_0001_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.prev(), some(anything()));
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&building_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_0003_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.prev_page().or_fail()?)
        .await?;
    expect_that!(page_3.prev(), none());
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&building_0000_id)));
    expect_that!(page_3.items(), nth(1, has_id(&building_0001_id)));
    Ok(())
}
