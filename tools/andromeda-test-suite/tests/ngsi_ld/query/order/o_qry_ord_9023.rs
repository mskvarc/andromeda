//! A missing attribute sorts last in both directions.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy},
};

/// Pins: `orderBy=name` and orderBy=name;desc both rank the Entity without name last
/// Clause: 4.23.2, 4.23.3
/// Recorded: 4.23.2 puts "Attribute does not exist" last with null values last; Andromeda keeps it last when the
/// direction is reversed
#[gtest]
#[tokio::test]
async fn o_qry_ord_9023_a_missing_attribute_sorts_last_in_both_directions() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_absent_a_id = namespace.urn("Building", "absent-a")?;
    let building_type = namespace.entity_type("Building")?;
    let building_absent_b_id = namespace.urn("Building", "absent-b")?;
    let building_absent_none_id = namespace.urn("Building", "absent-none")?;
    let entities = [
        Entity::builder()
            .id(building_absent_a_id.clone())
            .entity_type(building_type.clone())
            .property("name", "alpha")
            .build()?,
        Entity::builder()
            .id(building_absent_b_id.clone())
            .entity_type(building_type.clone())
            .property("name", "beta")
            .build()?,
        Entity::builder()
            .id(building_absent_none_id.clone())
            .entity_type(building_type.clone())
            .property("unrelated", 1)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![
            building_absent_a_id.clone(),
            building_absent_b_id.clone(),
            building_absent_none_id.clone(),
        ])
        .locality(Locality::Local)
        .ordering("name".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_absent_a_id)));
    expect_that!(page.items(), nth(1, has_id(&building_absent_b_id)));
    expect_that!(page.items(), nth(2, has_id(&building_absent_none_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![
            building_absent_a_id.clone(),
            building_absent_b_id.clone(),
            building_absent_none_id.clone(),
        ])
        .locality(Locality::Local)
        .ordering("name;desc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), nth(0, has_id(&building_absent_b_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_absent_a_id)));
    expect_that!(page_2.items(), nth(2, has_id(&building_absent_none_id)));
    Ok(())
}
