//! An explicit offset without local steps by offset.

use andromeda_test_suite::{
    broker,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    page_link::{carries_parameter, links},
    wire::header_values,
};
use googletest::prelude::*;
use http::header::LINK;
use ngsi_ld::{
    http::headers::{Accept, Link},
    model::{Entity, Input, NgsiLdMediaType},
};

/// Pins: Query Entities with limit 2 and offset 2 over five entities without local answers the third and fourth
/// entities with pointers at offsets 4 and 0 and no cursor
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 6.3.10 says the pointers of the transparent option "should include" limit and offset; Andromeda keeps
/// offset pointers when the request names an offset or an order
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0030_an_explicit_offset_without_local_steps_by_offset() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let building_0002_id = namespace.urn("Building", "0002")?;
    let building_0003_id = namespace.urn("Building", "0003")?;
    let entities = (0..=4)
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
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .limit(2)
        .offset(2)
        .send()
        .await?;
    let page = headed.outcome;
    expect_that!(header_values(headed.head.headers(), &LINK), len(eq(1)));
    expect_that!(
        headed
            .head
            .typed::<Link>()
            .valid()
            .map(|link| link.values().iter().map(|value| value.target().as_str().to_owned()).collect::<Vec<_>>()),
        some(contains(contains_substring("offset=4")))
    );
    expect_that!(
        headed
            .head
            .typed::<Link>()
            .valid()
            .map(|link| link.values().iter().map(|value| value.target().as_str().to_owned()).collect::<Vec<_>>()),
        some(contains(contains_substring("offset=0")))
    );
    expect_that!(links(&page), each(not(carries_parameter("cursor"))));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_0002_id)));
    expect_that!(page.items(), nth(1, has_id(&building_0003_id)));
    Ok(())
}
