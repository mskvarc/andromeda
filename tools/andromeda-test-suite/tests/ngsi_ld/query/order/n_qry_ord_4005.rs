//! Ordering by scope keeps an entity without a scope on the last page.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy=scope` with limit=2 answers the two lowest scopes on the first page, the third scope and the
/// entity without a scope on the second, and offset=3 answers the entity without a scope alone
/// Clause: 4.23.2, 4.23.3, 6.4.3.2, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_ord_4005_ordering_by_scope_keeps_an_entity_without_a_scope_on_the_last_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let zone_0_id = namespace.urn("Zone", "0")?;
    let building_type = namespace.entity_type("Building")?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder().id(zone_0_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .scope("/b")
            .build()?,
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .scope("/c")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .scope("/a")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering("scope".parse::<OrderBy>()?)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering("scope".parse::<OrderBy>()?)
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&building_id)));
    expect_that!(page_2.items(), nth(1, has_id(&zone_0_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering("scope".parse::<OrderBy>()?)
        .limit(2)
        .offset(3)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&zone_0_id)));
    Ok(())
}
