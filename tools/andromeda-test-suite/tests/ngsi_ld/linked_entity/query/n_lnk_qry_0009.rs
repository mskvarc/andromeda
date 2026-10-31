//! A query body contained by member keeps the listed entity out.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, type_matcher::has_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Entity, NgsiLdMediaType},
    query::Join,
};

/// Pins: a Query body's `containedBy` member keeps the listed entity out of a flat join, as the GET parameter does:
/// without it two hops answer a, b and c, with b listed b is not appended while a is answered
/// Clause: 4.5.23, 5.2.23, 5.7.2.5, 6.23.3.1
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0009_a_query_body_contained_by_member_keeps_the_listed_entity_out() -> Result<()> {
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
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_a_id)));
    expect_that!(page.items(), contains(all!(has_id(&building_b_id), has_type(&building_type))));
    expect_that!(page.items(), contains(all!(has_id(&building_c_id), has_type(&building_type))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .method(QueryMethod::Post)
        .ids(vec![building_a_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(2.try_into()?)
        .contained_by(vec![building_b_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), nth(0, has_id(&building_a_id)));
    expect_that!(page_2.items(), not(contains(has_id(&building_b_id))));
    Ok(())
}
