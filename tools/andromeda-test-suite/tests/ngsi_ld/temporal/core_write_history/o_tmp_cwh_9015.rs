//! A write leaving the location alone records no location instance.

use andromeda_test_suite::{broker, evolution_matcher::has_evolution, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        NgsiLdMediaType,
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::{Locality, geoquery, temporal_query::TemporalProperty},
};

/// Pins: Update Attributes of speed and Append Attributes of colour on an Entity whose location is a triangle
/// answer 204, the location keeps its one recorded instance and an intersects query inside the triangle still
/// selects the Entity; Update Attributes moving the location to Point `[3, 4]` answers 204, the location gains a
/// second instance, near Point `[3, 4]` selects the Entity and the triangle no longer does
/// Clause: 5.6.2, 5.6.3, 4.7, 4.10, 4.5.7, 4.3.1
/// Recorded: clause 4.3.1 leaves it to the broker whether a Core API write updates the Temporal Evolution ("should
/// consider updating"); Andromeda records Core writes by default
#[gtest]
#[tokio::test]
async fn o_tmp_cwh_9015_a_write_leaving_the_location_alone_records_no_location_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let parcel_hoisted_geometry_id = namespace.urn("Parcel", "hoisted-geometry")?;
    let parcel_type = namespace.entity_type("Parcel")?;
    let parcel_hoisted_geometry = Entity::builder()
        .id(parcel_hoisted_geometry_id.clone())
        .entity_type(parcel_type.clone())
        .property("speed", 1)
        .geo_property(
            "location",
            Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(vec![
                Position::try_new(10.0, 50.0)?,
                Position::try_new(10.1, 50.0)?,
                Position::try_new(10.1, 50.1)?,
                Position::try_new(10.0, 50.0)?,
            ])?])?),
        )
        .build()?;
    broker.entities().create(&parcel_hoisted_geometry).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 2).build()?;
    broker
        .entities()
        .update_attributes(&parcel_hoisted_geometry_id, &fragment)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&parcel_hoisted_geometry_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["location".parse()?])
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(evolution, has_evolution("location", len(eq(1))));
    let fragment_2 = EntityFragment::builder().property("colour", "red").build()?;
    broker
        .entities()
        .append_attributes(&parcel_hoisted_geometry_id, &fragment_2)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&parcel_hoisted_geometry_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["location".parse()?])
        .send()
        .await?;
    let evolution_2 = retrieved_2.body();
    expect_that!(evolution_2, has_evolution("location", len(eq(1))));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(10.08, 50.03)?)))
        .entity_type(&parcel_type)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&parcel_hoisted_geometry_id)));
    let fragment_3 = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_new(3.0, 4.0)?))
        .build()?;
    broker
        .entities()
        .update_attributes(&parcel_hoisted_geometry_id, &fragment_3)
        .send()
        .await?
        .done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&parcel_hoisted_geometry_id)
        .time_property(TemporalProperty::ModifiedAt)
        .attrs(vec!["location".parse()?])
        .send()
        .await?;
    let evolution_3 = retrieved_3.body();
    expect_that!(evolution_3, has_evolution("location", len(eq(2))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(3.0, 4.0)?)))
        .entity_type(&parcel_type)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), contains(has_id(&parcel_hoisted_geometry_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(10.08, 50.03)?)))
        .entity_type(&parcel_type)
        .locality(Locality::Local)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    Ok(())
}
