//! Limit caps the page size.

use andromeda_test_suite::{broker, namespace::Namespace, nth::nth, outcome::Done, type_matcher::has_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: limit=2 caps the page at two entities, on an id-and-type query over three Buildings and on a near
/// geo-query over five
/// Clause: 5.5.9.1, 6.3.10, 5.7.2, 4.10
#[gtest]
#[tokio::test]
async fn n_qry_pag_0001_limit_caps_the_page_size() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_g0_id = namespace.urn("Building", "g0")?;
    let building_g1_id = namespace.urn("Building", "g1")?;
    let building_g2_id = namespace.urn("Building", "g2")?;
    let building_g3_id = namespace.urn("Building", "g3")?;
    let building_g4_id = namespace.urn("Building", "g4")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "b1")
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "b2")
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("name", "b3")
            .build()?,
        Entity::builder()
            .id(building_g0_id.clone())
            .entity_type(building_type.clone())
            .property("name", "bg0")
            .geo_property("location", Shape::Point(Position::try_new(13.38893, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(building_g1_id.clone())
            .entity_type(building_type.clone())
            .property("name", "bg1")
            .geo_property("location", Shape::Point(Position::try_new(13.38993, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(building_g2_id.clone())
            .entity_type(building_type.clone())
            .property("name", "bg2")
            .geo_property("location", Shape::Point(Position::try_new(13.39093, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(building_g3_id.clone())
            .entity_type(building_type.clone())
            .property("name", "bg3")
            .geo_property("location", Shape::Point(Position::try_new(13.39193, 52.534_473)?))
            .build()?,
        Entity::builder()
            .id(building_g4_id.clone())
            .entity_type(building_type.clone())
            .property("name", "bg4")
            .geo_property("location", Shape::Point(Position::try_new(13.39293, 52.534_473)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .ids(vec![building_id.clone(), building_2_id.clone(), building_3_id.clone()])
        .entity_type(&building_type)
        .limit(2)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(
        page.items().first().map(|item| item.id.to_string()),
        some(contains_regex(format!("^urn:ngsi-ld:Building:{namespace}-[1-3]$")))
    );
    expect_that!(
        page.items().get(1).map(|item| item.id.to_string()),
        some(contains_regex(format!("^urn:ngsi-ld:Building:{namespace}-[1-3]$")))
    );
    expect_that!(page.items(), nth(0, has_type(&building_type)));
    expect_that!(page.items(), nth(1, has_type(&building_type)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .query()
        .geo_q(geoquery::near_max(15000, Shape::Point(Position::try_new(13.388_930, 52.534_473)?)))
        .entity_type(&building_type)
        .ids(vec![
            building_g0_id.clone(),
            building_g1_id.clone(),
            building_g2_id.clone(),
            building_g3_id.clone(),
            building_g4_id.clone(),
        ])
        .limit(2)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(
        page_2.items().first().map(|item| item.id.to_string()),
        some(contains_regex(format!("^urn:ngsi-ld:Building:{namespace}-g[0-4]$")))
    );
    expect_that!(
        page_2.items().get(1).map(|item| item.id.to_string()),
        some(contains_regex(format!("^urn:ngsi-ld:Building:{namespace}-g[0-4]$")))
    );
    Ok(())
}
