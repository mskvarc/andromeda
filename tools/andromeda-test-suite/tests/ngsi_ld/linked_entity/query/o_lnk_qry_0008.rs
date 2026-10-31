//! A held entity is not traversed.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with join=flat does not traverse an entity `containedBy` lists, so an entity reachable only
/// through it is not appended either: a -> b -> c under two hops with b held answers a alone
/// Clause: 4.5.23, 5.7.2.4, 5.7.2.5, 6.4.3.2
/// Recorded: 5.7.2.5 says only that a previously encountered URI is not appended and Table 6.4.3.2-1 that
/// `containedBy` lists the entities "previously encountered whilst retrieving the Entity Graph"; whether the
/// traversal continues through such an entity is left open. Andromeda stops at it, as the client already holds it
#[gtest]
#[tokio::test]
async fn o_lnk_qry_0008_a_held_entity_is_not_traversed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_c_id = namespace.urn("Building", "c")?;
    let building_type = namespace.entity_type("Building")?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let entities = [
        Entity::builder().id(building_c_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_b_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_c_id.clone())
            .build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_b_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // without containedBy the two hops reach b and c
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .contained_by(vec![building_b_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_a_id)));
    Ok(())
}
