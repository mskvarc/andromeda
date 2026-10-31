//! Counts every entity and reports only its own families.

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
        Property,
        geometry::{Position, Shape},
    },
};

/// Pins: the entity type information counts every entity carrying the type, a multi-typed entity included, and
/// reports for each attribute only the families the instances of that type's own entities declare
/// Clause: 5.7.7.4, 5.2.26, 4.16
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0003_counts_every_entity_and_reports_only_its_own_families() -> Result<()> {
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
    let building_information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(building_information.entity_count, eq(4));
    expect_that!(building_information.attribute_details, len(eq(3)));
    expect_that!(families_of(&building_information, &marker), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(
        families_of(&building_information, &owner),
        some(elements_are![eq(&AttributeKind::Relationship)])
    );
    expect_that!(
        families_of(&building_information, &position),
        some(elements_are![eq(&AttributeKind::GeoProperty)])
    );
    let store_information = broker.with_accept(Accept::any()).entity_types().retrieve(&store_type).send().await?;
    expect_that!(store_information.entity_count, eq(2));
    expect_that!(store_information.attribute_details, len(eq(1)));
    expect_that!(attribute_names(&store_information), elements_are![eq(&marker)]);
    expect_that!(
        families_of(&store_information, &marker),
        some(unordered_elements_are![eq(&AttributeKind::Property), eq(&AttributeKind::Relationship)])
    );
    Ok(())
}
