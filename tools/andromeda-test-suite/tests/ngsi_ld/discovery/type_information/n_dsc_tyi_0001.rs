//! An attribute a new entity brings joins the details.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    type_information::{attribute_names, families_of},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        AttributeKind,
        Entity,
        geometry::{Position, Shape},
    },
};

/// Pins: the entity type information lists every attribute an entity of the type carries with the family its
/// instances declare, and grows when a new entity of the type brings an attribute the others lack; the type
/// details, the attribute list and the attribute information follow
/// Clause: 5.7.7.4, 5.2.26, 5.7.6.4, 5.7.8.4, 5.7.10.4, 5.2.28
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0001_an_attribute_a_new_entity_brings_joins_the_details() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_y_id = namespace.urn("Building", "y")?;
    let building_z_id = namespace.urn("Building", "z")?;
    let building_type = namespace.entity_type("Building")?;
    let height = namespace.attribute("height")?;
    let owner = namespace.attribute("owner")?;
    let floors = namespace.attribute("floors")?;
    let position = namespace.attribute("position")?;
    let entities = [
        Entity::builder()
            .id(building_x_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 1)
            .relationship(namespace.attribute("owner")?, building_y_id.clone())
            .property(namespace.attribute("floors")?, 2)
            .build()?,
        Entity::builder()
            .id(building_z_id)
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 1)
            .relationship(namespace.attribute("owner")?, building_y_id.clone())
            .property(namespace.attribute("floors")?, 2)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.type_name, eq(&building_type));
    expect_that!(information.entity_count, eq(2));
    expect_that!(information.attribute_details, len(eq(3)));
    expect_that!(families_of(&information, &height), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(families_of(&information, &owner), some(elements_are![eq(&AttributeKind::Relationship)]));
    expect_that!(families_of(&information, &floors), some(elements_are![eq(&AttributeKind::Property)]));
    let building_y = Entity::builder()
        .id(building_y_id)
        .entity_type(building_type.clone())
        .property(namespace.attribute("height")?, 3)
        .relationship(namespace.attribute("owner")?, building_x_id)
        .property(namespace.attribute("floors")?, 4)
        .geo_property(namespace.attribute("position")?, Shape::Point(Position::try_new(14.5, 46.05)?))
        .build()?;
    broker.entities().create(&building_y).send().await?.done()?;
    let information_2 = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information_2.entity_count, eq(3));
    expect_that!(information_2.attribute_details, len(eq(4)));
    expect_that!(families_of(&information_2, &height), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(families_of(&information_2, &owner), some(elements_are![eq(&AttributeKind::Relationship)]));
    expect_that!(families_of(&information_2, &floors), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(families_of(&information_2, &position), some(elements_are![eq(&AttributeKind::GeoProperty)]));
    let details = broker.with_accept(Accept::any()).entity_types().list_details().send().await?;
    let building = details.iter().find(|item| item.type_name == building_type).or_fail()?;
    expect_that!(
        building.attribute_names,
        unordered_elements_are![eq(&height), eq(&owner), eq(&floors), eq(&position)]
    );
    expect_that!(
        attribute_names(&information_2),
        unordered_elements_are![eq(&height), eq(&owner), eq(&floors), eq(&position)]
    );
    let attributes = broker.with_accept(Accept::any()).attributes().list().send().await?;
    for name in [&height, &owner, &floors, &position] {
        expect_that!(attributes.attribute_list, contains(eq(name)));
    }
    let position_information = broker.with_accept(Accept::any()).attributes().retrieve(&position).send().await?;
    expect_that!(position_information.attribute_count, some(eq(1)));
    expect_that!(
        position_information.attribute_types.as_deref(),
        some(elements_are![eq(&AttributeKind::GeoProperty)])
    );
    expect_that!(position_information.type_names.as_deref(), some(elements_are![eq(&building_type)]));
    Ok(())
}
