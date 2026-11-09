//! An entity created mid walk ahead of it appears in the walk.

use andromeda_test_suite::{broker, cursor_walk, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryEntities},
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{Locality, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities with limit 2 and an empty cursor over four entities, with an entity of a greater identifier
/// created after the first page, answers that entity on a later page of the walk its next pointers continue
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.1 lets an implementation detect changes to the result set during pagination ("may"); Andromeda
/// continues a cursor walk after the last identifier seen, so changes behind or ahead of the walk shift no page
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0023_an_entity_created_mid_walk_ahead_of_it_appears_in_the_walk() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0001_id = namespace.urn("Building", "0001")?;
    let building_0002_id = namespace.urn("Building", "0002")?;
    let building_0003_id = namespace.urn("Building", "0003")?;
    let building_9999_id = namespace.urn("Building", 9999)?;
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
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&building_0001_id)));
    let building_9999 = Entity::builder()
        .id(building_9999_id.clone())
        .entity_type(building_type.clone())
        .property("name", "b9999")
        .build()?;
    broker.entities().create(&building_9999).send().await?.done()?;
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&building_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_0003_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.next_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&building_9999_id)));
    Ok(())
}
