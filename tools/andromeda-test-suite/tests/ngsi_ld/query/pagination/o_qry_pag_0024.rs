//! An entity deleted behind the walk shifts no page.

use andromeda_test_suite::{broker, cursor_walk, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryEntities},
    model::{Entity, Input, Lossless, NgsiLdMediaType},
    query::{Locality, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities with limit 2 and an empty cursor over six entities, with the first entity deleted after the
/// first page, answers the third and fourth entities on the page its next pointer leads to
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.1 lets an implementation detect changes to the result set during pagination ("may"); Andromeda
/// continues a cursor walk after the last identifier seen, so changes behind or ahead of the walk shift no page
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0024_an_entity_deleted_behind_the_walk_shifts_no_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_0000_id = namespace.urn("Building", "0000")?;
    let building_type = namespace.entity_type("Building")?;
    let building_0001_id = namespace.urn("Building", "0001")?;
    let building_0002_id = namespace.urn("Building", "0002")?;
    let building_0003_id = namespace.urn("Building", "0003")?;
    let entities = (0..=5)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Building", format!("{n:04}"))?)
                .entity_type(building_type.clone())
                .property("name", format!("b{n}"))
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    for entity in &entities {
        broker.entities().create(entity).send().await?.done()?;
    }
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
    broker.entities().delete(&building_0000_id).send().await?.done()?;
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&building_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_0003_id)));
    Ok(())
}
