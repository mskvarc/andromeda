//! A geo property with a non core name reads back as a geo property.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
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

/// Pins: a `GeoProperty` under a name the core @context does not define is created and reads back as a
/// `GeoProperty` with its Point geometry
/// Clause: 5.6.1, 4.7, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0005_a_geo_property_with_a_non_core_name_reads_back_as_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .geo_property("customLocation", Shape::Point(Position::try_new(13.3986, 52.5547)?))
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
            "customLocation",
            geo_property()
                .value_member(["type"], "Point")
                .value_member(["coordinates", "0"], Value::Number("13.3986".parse()?))
                .value_member(["coordinates", "1"], Value::Number("52.5547".parse()?))
        )
    );
    Ok(())
}
