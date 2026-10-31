//! Following next returns every entity once and the last page points back.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Query Entities with limit 2 over five entities, followed by its next pointers, answers three pages holding
/// every entity once, the last with a prev pointer and no next
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn n_qry_pag_0013_following_next_returns_every_entity_once_and_the_last_page_points_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .limit(2)
        .send()
        .await?;
    let a_id = page.items().first().or_fail()?.id.clone();
    let b_id = page.items().get(1).or_fail()?.id.clone();
    expect_that!(page.items(), len(eq(2)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page.next_page().or_fail()?)
        .await?;
    let c_id = page_2.items().first().or_fail()?.id.clone();
    let d_id = page_2.items().get(1).or_fail()?.id.clone();
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), not(contains(has_id(&a_id))));
    expect_that!(page_2.items(), not(contains(has_id(&b_id))));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .send(&page_2.next_page().or_fail()?)
        .await?;
    expect_that!(page_3.prev(), some(anything()));
    expect_that!(page_3.next(), none());
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, not(has_id(&a_id))));
    expect_that!(page_3.items(), nth(0, not(has_id(&b_id))));
    expect_that!(page_3.items(), nth(0, not(has_id(&c_id))));
    expect_that!(page_3.items(), nth(0, not(has_id(&d_id))));
    Ok(())
}
