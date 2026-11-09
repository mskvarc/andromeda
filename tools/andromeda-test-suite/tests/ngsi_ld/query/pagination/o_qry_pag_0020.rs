//! The prev pointer stops at a short backward page.

use andromeda_test_suite::{broker, cursor_walk, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryEntities},
    model::{Entity, Lossless, NgsiLdMediaType},
    query::{Locality, builders::SelectorTypeBuilder},
};

/// Pins: Query Entities with limit 3 and an empty cursor over five entities offers a last pointer to the final three;
/// that page's prev pointer leads to the first two entities, and that short page offers no prev pointer
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0020_the_prev_pointer_stops_at_a_short_backward_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let mut ids = Vec::new();
    for n in 0..5 {
        let id = namespace.urn("Building", format!("{n:04}"))?;
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("name", format!("b{n}"))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
        ids.push(id);
    }
    let json_ld = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let selector = SelectorTypeBuilder::from(&building_type).build()?;
    let query = QueryEntities::builder()
        .entity_type(&selector)
        .locality(Locality::Local)
        .limit(3)
        .format(Lossless::Normalized)
        .build();
    let first = cursor_walk::first_page(&json_ld, &query).await?.page;
    let last = json_ld.send(&first.last_page().or_fail()?).await?;
    expect_that!(last.items(), ids_in_order(&ids[2..]));
    let previous = json_ld.send(&last.prev_page().or_fail()?).await?;
    expect_that!(previous.items(), ids_in_order(&ids[..2]));
    expect_that!(previous.prev(), none());
    expect_that!(previous.next(), some(anything()));
    Ok(())
}
