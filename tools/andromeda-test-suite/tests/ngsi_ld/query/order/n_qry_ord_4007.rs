//! An explicit asc direction orders ascending.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Locality, ordering::OrderBy},
};

/// Pins: orderBy=scope;asc and orderBy=name;asc order the entities ascending, the direction asc being admitted like
/// the default
/// Clause: 4.23.3, 6.4.3.2
/// Recorded: 4.23.3 gives "`directionOp` `::=` asc | desc | dist-asc | dist-desc" and "`DirectionTerm` = %x3B
/// `directionOp`", so ;asc is a valid direction term
#[gtest]
#[tokio::test]
async fn n_qry_ord_4007_an_explicit_asc_direction_orders_ascending() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .scope("/c")
            .property("name", "Zulu")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .scope("/a")
            .property("name", "Mike")
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .scope("/b")
            .property("name", "Alpha")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering("scope;asc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), nth(0, has_id(&building_2_id)));
    expect_that!(page.items(), nth(1, has_id(&building_3_id)));
    expect_that!(page.items(), nth(2, has_id(&building_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .locality(Locality::Local)
        .ordering("name;asc".parse::<OrderBy>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(3)));
    expect_that!(page_2.items(), nth(0, has_id(&building_3_id)));
    expect_that!(page_2.items(), nth(1, has_id(&building_2_id)));
    expect_that!(page_2.items(), nth(2, has_id(&building_id)));
    Ok(())
}
