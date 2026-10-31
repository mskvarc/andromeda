//! Following prev from the second page returns the first page.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    page_link::{carries_parameter, links},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Query Entities with limit 2 over six members answers a first page whose next pointer leads to a full
/// second page, and that page's prev pointer leads back to the members of the first page
/// Clause: 5.5.9.1, 6.3.10
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor and offers first and last pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_qry_pag_0019_following_prev_from_the_second_page_returns_the_first_page() -> Result<()> {
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    expect_that!(links(&page), contains(carries_parameter("cursor")));
    expect_that!(links(&page), each(not(carries_parameter("offset"))));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_0000_id)));
    expect_that!(page.items(), nth(1, has_id(&building_0001_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&building_0002_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_0003_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.prev_page().or_fail()?)
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), nth(0, has_id(&building_0000_id)));
    expect_that!(page_3.items(), nth(1, has_id(&building_0001_id)));
    Ok(())
}
