//! A geo query answers from the current location.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        GeoProperty,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::geoquery,
};

/// Pins: A geo-query answers from the current location: an appended dataset instance, a deleted entity, a run of
/// updates and a move are each reflected by the next query
/// Clause: 4.10, 5.6.2, 5.6.3, 5.6.6
#[gtest]
#[tokio::test]
async fn n_qry_geo_0017_a_geo_query_answers_from_the_current_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let kiosk_id = namespace.urn("Kiosk", 1)?;
    let kiosk_type = namespace.entity_type("Kiosk")?;
    let shed_id = namespace.urn("Shed", 1)?;
    let shed_type = namespace.entity_type("Shed")?;
    let booth_id = namespace.urn("Booth", 1)?;
    let booth_type = namespace.entity_type("Booth")?;
    let stall_id = namespace.urn("Stall", 1)?;
    let stall_type = namespace.entity_type("Stall")?;
    let entities = [
        Entity::builder()
            .id(kiosk_id.clone())
            .entity_type(kiosk_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(20.0, 40.0)?))
            .build()?,
        Entity::builder()
            .id(shed_id.clone())
            .entity_type(shed_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
        Entity::builder()
            .id(booth_id.clone())
            .entity_type(booth_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
        Entity::builder()
            .id(stall_id.clone())
            .entity_type(stall_type.clone())
            .geo_property("location", Shape::Point(Position::try_new(0.5, 0.5)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // Append: the Kiosk is outside until a dataset instance inside is appended.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.0, 52.0)?,
            Position::try_new(14.0, 52.0)?,
            Position::try_new(14.0, 53.0)?,
            Position::try_new(13.0, 53.0)?,
            Position::try_new(13.0, 52.0)?,
        ])?])?)))
        .entity_type(&kiosk_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let fragment = EntityFragment::builder()
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.4, 52.52)?)).dataset_id(namespace.dataset_id("Dataset", "gps")?),
        )
        .build()?;
    broker.entities().append_attributes(&kiosk_id, &fragment).send().await?.done()?;
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(13.0, 52.0)?,
            Position::try_new(14.0, 52.0)?,
            Position::try_new(14.0, 53.0)?,
            Position::try_new(13.0, 53.0)?,
            Position::try_new(13.0, 52.0)?,
        ])?])?)))
        .entity_type(&kiosk_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&kiosk_id)));
    // Delete: a deleted entity is not found.
    broker.entities().delete(&shed_id).send().await?.done()?;
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&shed_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    // Rapid updates: five location changes in a row.
    let fragment_2 = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(10.0, 10.0)?))
        .build()?;
    broker.entities().update_attributes(&booth_id, &fragment_2).send().await?.done()?;
    let fragment_3 = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(10.1, 10.0)?))
        .build()?;
    broker.entities().update_attributes(&booth_id, &fragment_3).send().await?.done()?;
    let fragment_4 = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(10.2, 10.0)?))
        .build()?;
    broker.entities().update_attributes(&booth_id, &fragment_4).send().await?.done()?;
    let fragment_5 = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(10.3, 10.0)?))
        .build()?;
    broker.entities().update_attributes(&booth_id, &fragment_5).send().await?.done()?;
    let fragment_6 = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(10.4, 10.0)?))
        .build()?;
    broker.entities().update_attributes(&booth_id, &fragment_6).send().await?.done()?;
    // The last position, 10.4/10.0, is inside.
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(10.0, 9.5)?,
            Position::try_new(10.5, 9.5)?,
            Position::try_new(10.5, 10.5)?,
            Position::try_new(10.0, 10.5)?,
            Position::try_new(10.0, 9.5)?,
        ])?])?)))
        .entity_type(&booth_type)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&booth_id)));
    // Move: found at 0.5/0.5 ...
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&stall_type)
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), nth(0, has_id(&stall_id)));
    let fragment_7 = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(10.0, 10.0)?))
        .build()?;
    broker.entities().update_attributes(&stall_id, &fragment_7).send().await?.done()?;
    // ... no longer found there after moving to 10/10 ...
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(0.0, 0.0)?,
            Position::try_new(1.0, 0.0)?,
            Position::try_new(1.0, 1.0)?,
            Position::try_new(0.0, 1.0)?,
            Position::try_new(0.0, 0.0)?,
        ])?])?)))
        .entity_type(&stall_type)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    // ... and found around its new position.
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::within(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
            Position::try_new(9.0, 9.0)?,
            Position::try_new(11.0, 9.0)?,
            Position::try_new(11.0, 11.0)?,
            Position::try_new(9.0, 11.0)?,
            Position::try_new(9.0, 9.0)?,
        ])?])?)))
        .entity_type(&stall_type)
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_id(&stall_id)));
    Ok(())
}
