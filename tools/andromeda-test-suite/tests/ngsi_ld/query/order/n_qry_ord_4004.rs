//! Ordering by scope ranks an entity without a scope last.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy, type_selection},
};

/// Pins: `orderBy=scope` answers the entities with a scope in ascending scope order and an entity without a scope
/// last when the request names several types
/// Clause: 4.23.2, 4.23.3, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_qry_ord_4004_ordering_by_scope_ranks_an_entity_without_a_scope_last() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let zone_0_id = namespace.urn("Zone", "0")?;
    let zone_type = namespace.entity_type("Zone")?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_type = namespace.entity_type("Building")?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let entities = [
        Entity::builder().id(zone_0_id.clone()).entity_type(zone_type.clone()).build()?,
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
    // The comparison order ends with "Null" and then "Attribute does not exist"; ascending is the default.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(type_selection::entity_type(&building_type).or(&zone_type))
        .locality(Locality::Local)
        .ordering("scope".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(4)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    expect_that!(page.items(), nth(2, has_id(&building_id)));
    expect_that!(page.items(), nth(3, has_id(&zone_0_id)));
    Ok(())
}
