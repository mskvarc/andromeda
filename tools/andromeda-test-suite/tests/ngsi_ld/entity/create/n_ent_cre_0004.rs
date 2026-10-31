//! A location geo property reads back beside the other properties.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, geo_property},
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: an entity created with a location `GeoProperty` (a Point) beside two Properties reads back with the
/// `GeoProperty`, its geometry type and the Properties' values
/// Clause: 5.6.1, 4.5.2, 4.7, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0004_a_location_geo_property_reads_back_beside_the_other_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .geo_property("location", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(
        entity,
        has_attribute(
            "location",
            geo_property()
                .value_member(["type"], "Point")
                .value_member(["coordinates", "0"], Value::Number("13.3986".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.5547".parse()?))
        )
    );
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    Ok(())
}
