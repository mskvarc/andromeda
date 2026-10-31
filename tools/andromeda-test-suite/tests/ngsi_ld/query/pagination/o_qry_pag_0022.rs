//! The last pointer leads to the final page.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Query Entities with limit 2 over five members offers a last pointer leading to a page of two members
/// holding the greatest identifier, with no next pointer
/// Clause: 5.5.9.1, 5.5.9.2, 6.3.10
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor and offers first and last pointers beside next and prev
#[gtest]
#[tokio::test]
async fn o_qry_pag_0022_the_last_pointer_leads_to_the_final_page() -> Result<()> {
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
    let first = json_ld
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let last = json_ld.send(&first.last_page().or_fail()?).await?;
    expect_that!(last.items(), len(eq(2)));
    expect_that!(last.items(), contains(has_id(&ids[4])));
    expect_that!(last.next(), none());
    Ok(())
}
