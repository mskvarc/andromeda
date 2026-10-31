//! Flat appends linked entities in relationship order.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with join=flat answers the matching entity first, then the Linked Entities in the order of
/// the Relationships that reach them (link1 before link2), identically on repeated requests
/// Clause: 4.5.23.3, 5.7.2.5
/// Recorded: 4.5.23.3 and 5.7.2.5 append the Linked Entities after the matching entities but fix no order among
/// them; Andromeda appends them in the member order it holds the Relationships in
#[gtest]
#[tokio::test]
async fn o_lnk_qry_0011_flat_appends_linked_entities_in_relationship_order() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_type = namespace.entity_type("Building")?;
    let building_c_id = namespace.urn("Building", "c")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let entities = [
        Entity::builder().id(building_b_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_c_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_a_id.clone())
            .entity_type(building_type.clone())
            .relationship("link1", building_b_id.clone())
            .relationship("link2", building_c_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_a_id)));
    expect_that!(page.items(), nth(1, has_id(&building_b_id)));
    expect_that!(page.items(), nth(2, has_id(&building_c_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), nth(0, has_id(&building_a_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_b_id)));
    expect_that!(page_2.items(), nth(2, has_id(&building_c_id)));
    Ok(())
}
