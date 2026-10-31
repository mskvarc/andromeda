//! Families follow a change of attribute family.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    type_information::{attribute_names, families_of},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, EntityFragment},
};

/// Pins: when one entity's Property is replaced by a Relationship of the same name the type reports both families,
/// and once every entity's instance is a Relationship only that family is reported, by the type and by the
/// attribute
/// Clause: 5.7.7.4, 5.2.26, 5.7.10.4, 5.6.3.4
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0004_families_follow_a_change_of_attribute_family() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_z_id = namespace.urn("Building", "z")?;
    let building_type = namespace.entity_type("Building")?;
    let marker = namespace.attribute("marker")?;
    let entities = [
        Entity::builder()
            .id(building_x_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 1)
            .build()?,
        Entity::builder()
            .id(building_z_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("marker")?, 2)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let to_z = EntityFragment::builder()
        .relationship(namespace.attribute("marker")?, building_z_id.clone())
        .build()?;
    broker.entities().append_attributes(&building_x_id, &to_z).send().await?.done()?;
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.entity_count, eq(2));
    expect_that!(information.attribute_details, len(eq(1)));
    expect_that!(attribute_names(&information), elements_are![eq(&marker)]);
    expect_that!(
        families_of(&information, &marker),
        some(unordered_elements_are![eq(&AttributeKind::Property), eq(&AttributeKind::Relationship)])
    );
    let to_x = EntityFragment::builder().relationship(namespace.attribute("marker")?, building_x_id).build()?;
    broker.entities().append_attributes(&building_z_id, &to_x).send().await?.done()?;
    let information_2 = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information_2.entity_count, eq(2));
    expect_that!(information_2.attribute_details, len(eq(1)));
    expect_that!(families_of(&information_2, &marker), some(elements_are![eq(&AttributeKind::Relationship)]));
    let attribute_information = broker.with_accept(Accept::any()).attributes().retrieve(&marker).send().await?;
    expect_that!(attribute_information.attribute_count, some(eq(2)));
    expect_that!(
        attribute_information.attribute_types.as_deref(),
        some(elements_are![eq(&AttributeKind::Relationship)])
    );
    expect_that!(attribute_information.type_names.as_deref(), some(elements_are![eq(&building_type)]));
    Ok(())
}
