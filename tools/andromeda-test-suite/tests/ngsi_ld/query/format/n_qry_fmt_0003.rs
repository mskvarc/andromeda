//! Post geojson answers a feature collection.

use andromeda_test_suite::{
    attribute_matcher::{geo_property, property, relationship},
    broker,
    entity_matcher::{has_attribute, member_count},
    geometry_matcher::geometry,
    namespace::Namespace,
    outcome::Done,
    type_matcher::has_type,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{
        Entity,
        NgsiLdMediaType,
        Property,
        Relationship,
        Value,
        geometry::{Position, Shape},
    },
    query::type_selection,
};

/// Pins: Query Entities by POST as geo+json answers a `FeatureCollection` with one Feature per entity of the
/// selected types, a null geometry for the entity without a `GeoProperty`
/// Clause: 4.5.16.3, 6.23
#[gtest]
#[tokio::test]
async fn n_qry_fmt_0003_post_geojson_answers_a_feature_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_id = namespace.urn("OffStreetParking", 1)?;
    let parking_type = namespace.entity_type("OffStreetParking")?;
    let downtown_id = namespace.urn("OffStreetParking", "Downtown1")?;
    let bob_id = namespace.urn("Person", "Bob")?;
    let camera_id = namespace.urn("Camera", "C1")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(downtown_id.clone())
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", bob_id.clone()),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let parking = Entity::builder()
        .id(parking_id.clone())
        .entity_type(parking_type.clone())
        .property("name", "Downtown One")
        .property(
            "availableSpotNumber",
            Property::builder(121)
                .observed_at("2017-07-29T12:05:02Z".parse()?)
                .property("reliability", Value::Number("0.7".parse()?))
                .relationship("providedBy", camera_id.clone()),
        )
        .property("totalSpotNumber", 200)
        .geo_property("location", Shape::Point(Position::try_new(-8.5, 41.2)?))
        .build()?;
    broker.entities().create(&parking).send().await?.done()?;
    // The model joins the alternatives of a type selection with `|`, where the Query body was written with `,`.
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::GeoJson))
        .entities()
        .query_geo_json()
        .method(QueryMethod::Post)
        .entity_type(type_selection::entity_type(&vehicle_type).or(&parking_type))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    let vehicle_feature = page.items().iter().find(|feature| feature.entity.id == vehicle_id).or_fail()?;
    expect_that!(vehicle_feature.geometry, none());
    expect_that!(&vehicle_feature.entity, has_type(&vehicle_type));
    expect_that!(member_count(&vehicle_feature.entity), eq(4));
    expect_that!(&vehicle_feature.entity, has_attribute("brandName", property().value("Mercedes")));
    expect_that!(
        &vehicle_feature.entity,
        has_attribute(
            "isParked",
            relationship()
                .object(downtown_id)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .sub_attribute("providedBy", relationship().object(bob_id))
        )
    );
    let parking_feature = page.items().iter().find(|feature| feature.entity.id == parking_id).or_fail()?;
    expect_that!(
        parking_feature.geometry.as_ref(),
        some(
            geometry()
                .of_type("Point")
                .length([], 2)
                .ordinate([0], Value::Number("-8.5".parse()?))
                .ordinate([1], Value::Number("41.2".parse()?))
        )
    );
    expect_that!(&parking_feature.entity, has_type(&parking_type));
    expect_that!(member_count(&parking_feature.entity), eq(6));
    expect_that!(&parking_feature.entity, has_attribute("name", property().value("Downtown One")));
    expect_that!(
        &parking_feature.entity,
        has_attribute(
            "availableSpotNumber",
            property()
                .value(121)
                .observed_at("2017-07-29T12:05:02Z".parse()?)
                .sub_attribute("reliability", property().value(Value::Number("0.7".parse()?)))
                .sub_attribute("providedBy", relationship().object(camera_id))
        )
    );
    expect_that!(&parking_feature.entity, has_attribute("totalSpotNumber", property().value(200)));
    expect_that!(
        &parking_feature.entity,
        has_attribute("location", geo_property().geometry(Shape::Point(Position::try_new(-8.5, 41.2)?)))
    );
    Ok(())
}
