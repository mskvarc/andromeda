//! Updating a two dimensional location to three dimensions keeps it searchable.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityFragment,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: Update Attributes replaces a two-ordinate location with a three-ordinate one, which reads back with its
/// altitude, and a near query with a two-ordinate reference still selects the entity
/// Clause: 4.7.1, 4.10, 5.6.2, 5.7.2
/// Recorded: the altitude read back is 4.7.1 behaviour, but the near match of a 400 m high Point by a two-ordinate
/// reference rests on near ignoring altitude: clause 4.10 defers the geometric relationships to OGC 06-103r4, whose
/// clause 6.1.2.5 says spatial operations work in the "map geometry" of the data and "will therefore not reflect z
/// or m values in calculations" and whose clause 6.1.15.1 defines the relationships by the projection of the
/// objects onto the horizontal surface, so a georel that ignores the altitude is the specified behaviour; the user
/// ruled this on 2026-10-06
#[gtest]
#[tokio::test]
async fn n_qry_geo_0081_updating_a_two_dimensional_location_to_three_dimensions_keeps_it_searchable() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .geo_property("location", Shape::Point(Position::try_new(13.4, 52.52)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragment = EntityFragment::builder()
        .geo_property("location", Shape::Point(Position::try_with_altitude(13.4, 52.52, 400.0)?))
        .build()?;
    broker.entities().update_attributes(&building_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "location",
            any_attribute()
                .length_at(["coordinates"], 3)
                .value_member(["coordinates", "0"], Value::Number("13.4".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.52".parse()?))
                .value_member(["coordinates", "2"], 400)
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .geo_q(geoquery::near_max(100, Shape::Point(Position::try_new(13.4, 52.52)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_id)));
    Ok(())
}
