//! Lists under each entry the attributes its own entities carry.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        Property,
        geometry::{Position, Shape},
    },
};

/// Pins: Retrieve Details of Available Entity Types names under each type exactly the attributes its own entities
/// carry, a multi-typed entity's attributes under each of its types, and never the entity scope
/// Clause: 5.7.6.4, 5.2.25, 4.16, 6.25.3.1
#[gtest]
#[tokio::test]
async fn n_dsc_tyd_0001_lists_under_each_entry_the_attributes_its_own_entities_carry() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let store_3_id = namespace.urn("Store", 3)?;
    let store_4_id = namespace.urn("Store", 4)?;
    let building_5_id = namespace.urn("Building", 5)?;
    let building_type = namespace.entity_type("Building")?;
    let store_type = namespace.entity_type("Store")?;
    let marker = namespace.attribute("marker")?;
    let owner = namespace.attribute("owner")?;
    let position = namespace.attribute("position")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 1)
            .relationship(namespace.attribute("owner")?, building_2_id.clone())
            .build()?,
        Entity::builder()
            .id(building_2_id)
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 2)
            .relationship(namespace.attribute("owner")?, building_id.clone())
            .geo_property(namespace.attribute("position")?, Shape::Point(Position::try_new(14.5, 46.05)?))
            .build()?,
        Entity::builder()
            .id(store_3_id)
            .entity_type(store_type.clone())
            .relationship(namespace.attribute("marker")?, building_id)
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
    let details = broker.with_accept(Accept::any()).entity_types().list_details().send().await?;
    let building = details.iter().find(|item| item.type_name == building_type).or_fail()?;
    expect_that!(building.attribute_names, unordered_elements_are![eq(&marker), eq(&owner), eq(&position)]);
    let store = details.iter().find(|item| item.type_name == store_type).or_fail()?;
    expect_that!(store.attribute_names, elements_are![eq(&marker)]);
    Ok(())
}
