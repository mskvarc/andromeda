//! A temporal q and a geo-query narrow the represented instances.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, geo_property},
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        GeoProperty,
        NgsiLdMediaType,
        Property,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
    query::{geoquery, q, temporal_query},
};

/// Pins: over a Vehicle observed inside a 12:00 to 13:00 window with speed 30 and 60, brandName BMW and Audi, and a
/// location far from and then near a reference Point, Query Temporal Evolution of Entities with q speed>50 represents
/// speed by the instance 60 alone and keeps both brandName and both location instances, and with a geo-query near
/// the reference represents location by the near instance alone and keeps both speed instances; the representation
/// includes only the Attribute instances which meet the temporal, query and geoquery restrictions, and an instance of
/// an Attribute the query or geo-query names meets them when the condition holds with that Attribute reduced to it
/// Clause: 5.7.4.4, 4.9, 4.10, 4.11
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9384_a_temporal_q_and_a_geo_query_narrow_the_represented_instances() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let far = || -> Result<Shape> { Ok(Shape::Point(Position::try_new(2.35, 42.22)?)) };
    let near = || -> Result<Shape> { Ok(Shape::Point(Position::try_new(-8.5, 41.2)?)) };
    let evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", Property::builder("BMW").observed_at("2018-08-01T12:01:00Z".parse()?))
        .property("brandName", Property::builder("Audi").observed_at("2018-08-01T12:02:00Z".parse()?))
        .property("speed", Property::builder(30).observed_at("2018-08-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(60).observed_at("2018-08-01T12:05:00Z".parse()?))
        .geo_property("location", GeoProperty::builder(far()?).observed_at("2018-08-01T12:04:00Z".parse()?))
        .geo_property("location", GeoProperty::builder(near()?).observed_at("2018-08-01T12:06:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    let by_speed = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .q(q::attr("speed").gt(50))
        .send()
        .await?;
    expect_that!(by_speed.items(), len(eq(1)));
    expect_that!(by_speed.items(), nth(0, has_id(&vehicle_id)));
    expect_that!(
        by_speed.items(),
        nth(0, has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(60)))))
    );
    expect_that!(by_speed.items(), nth(0, has_evolution("brandName", len(eq(2)))));
    expect_that!(by_speed.items(), nth(0, has_evolution("location", len(eq(2)))));
    let by_location = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2018-08-01T12:00:00Z".parse()?, "2018-08-01T13:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .geo_q(geoquery::near_max(2000, Shape::Point(Position::try_new(-8.503, 41.202)?)))
        .send()
        .await?;
    expect_that!(by_location.items(), len(eq(1)));
    expect_that!(by_location.items(), nth(0, has_id(&vehicle_id)));
    expect_that!(
        by_location.items(),
        nth(0, has_evolution("location", all!(len(eq(1)), nth(0, geo_property().geometry(near()?)))))
    );
    expect_that!(by_location.items(), nth(0, has_evolution("speed", len(eq(2)))));
    Ok(())
}
