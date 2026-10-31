//! Contained by keeps the listed entities out of a flat join.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, type_matcher::has_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with join=flat does not append an entity `containedBy` lists as previously encountered, for
/// one id and for several, while the matching entity is answered; without `containedBy` the same join appends them
/// Clause: 4.5.23, 5.7.2.4, 5.7.2.5, 6.4.3.2
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0007_contained_by_keeps_the_listed_entities_out_of_a_flat_join() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_c_id = namespace.urn("Building", "c")?;
    let building_type = namespace.entity_type("Building")?;
    let building_b_id = namespace.urn("Building", "b")?;
    let building_a_id = namespace.urn("Building", "a")?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_y_id = namespace.urn("Building", "y")?;
    let building_w_id = namespace.urn("Building", "w")?;
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
        Entity::builder().id(building_x_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder().id(building_y_id.clone()).entity_type(building_type.clone()).build()?,
        Entity::builder()
            .id(building_w_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_x_id.clone())
            .relationship("alsoLinkedTo", building_y_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // chain a -> b -> c under two hops, b already held: a comes back, b is not appended
    let page = broker
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
    expect_that!(page.items(), nth(0, has_id(&building_a_id)));
    expect_that!(page.items(), not(contains(has_id(&building_b_id))));
    // w links x and y, both held: only w comes back
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_w_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .contained_by(vec![building_x_id.clone(), building_y_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&building_w_id)));
    // the same join without containedBy appends both, so the exclusion above is the parameter's doing
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_w_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(3)));
    expect_that!(page_3.items(), nth(0, has_id(&building_w_id)));
    expect_that!(page_3.items(), contains(all!(has_id(&building_x_id), has_type(&building_type))));
    expect_that!(page_3.items(), contains(all!(has_id(&building_y_id), has_type(&building_type))));
    Ok(())
}
