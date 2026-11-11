//! Names every type whose entities carry it.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeName,
        Entity,
        Property,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Details of Available Attributes names for each attribute exactly the types of the entities
/// carrying it, both types of a multi-typed entity included
/// Clause: 5.7.9.4, 5.2.28, 4.16, 6.27.3.1
#[gtest]
#[tokio::test]
async fn n_dsc_atd_0001_names_every_type_whose_entities_carry_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let store_3_id = namespace.urn("Store", 3)?;
    let store_type = namespace.entity_type("Store")?;
    let store_4_id = namespace.urn("Store", 4)?;
    let building_5_id = namespace.urn("Building", 5)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 1)
            .relationship(namespace.attribute("owner")?, building_2_id.clone())
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 2)
            .relationship(namespace.attribute("owner")?, building_id.clone())
            .geo_property(namespace.attribute("position")?, Shape::Point(Position::try_new(14.5, 46.05)?))
            .build()?,
        Entity::builder()
            .id(store_3_id)
            .entity_type(store_type.clone())
            .relationship(namespace.attribute("marker")?, building_id.clone())
            .build()?,
        Entity::builder()
            .id(store_4_id)
            .entity_type(vec![building_type.clone(), store_type.clone()])
            .property(namespace.attribute("marker")?, 4)
            .property(
                namespace.attribute("marker")?,
                Property::builder(5).dataset_id(namespace.dataset_id("Dataset", "x")?),
            )
            .build()?,
        Entity::builder()
            .id(building_5_id)
            .entity_type(building_type.clone())
            .scope(format!("/Census/{namespace}").as_str())
            .property(namespace.attribute("marker")?, 6)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let attribute_details = broker.with_accept(Accept::any()).attributes().list_details().send().await?;
    let detailed_name: AttributeName = format!("marker_{namespace}").parse()?;
    let detailed = attribute_details.iter().find(|item| item.attribute_name == detailed_name).or_fail()?;
    expect_that!(detailed.type_names.as_ref().map(Vec::len), some(eq(2)));
    expect_that!(detailed.type_names.as_deref(), some(contains(eq(&building_type))));
    expect_that!(detailed.type_names.as_deref(), some(contains(eq(&store_type))));
    let detailed_name_2: AttributeName = format!("owner_{namespace}").parse()?;
    let detailed_2 = attribute_details.iter().find(|item| item.attribute_name == detailed_name_2).or_fail()?;
    expect_that!(detailed_2.type_names.as_ref().map(Vec::len), some(eq(1)));
    expect_that!(detailed_2.type_names.as_deref().and_then(|names| names.first()), some(eq(&building_type)));
    let detailed_name_3: AttributeName = format!("position_{namespace}").parse()?;
    let detailed_3 = attribute_details.iter().find(|item| item.attribute_name == detailed_name_3).or_fail()?;
    expect_that!(detailed_3.type_names.as_ref().map(Vec::len), some(eq(1)));
    expect_that!(detailed_3.type_names.as_deref().and_then(|names| names.first()), some(eq(&building_type)));
    Ok(())
}
