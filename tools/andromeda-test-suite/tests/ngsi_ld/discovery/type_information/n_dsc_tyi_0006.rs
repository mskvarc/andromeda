//! A merge adding it counts the merged entity.

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

/// Pins: a Merge Entity whose fragment names an additional type adds the type to the entity, so the new type is
/// available with that entity and its attributes, and the attribute names both types
/// Clause: 5.7.7.4, 5.7.10.4, 5.6.17.4, 5.5.12, 4.16
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0006_a_merge_adding_it_counts_the_merged_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_type = namespace.entity_type("Building")?;
    let store_type = namespace.entity_type("Store")?;
    let height = namespace.attribute("height")?;
    let building_x = Entity::builder()
        .id(building_x_id.clone())
        .entity_type(building_type.clone())
        .property(namespace.attribute("height")?, 1)
        .build()?;
    broker.entities().create(&building_x).send().await?.done()?;
    let fragment = EntityFragment::builder().entity_type(vec![building_type.clone(), store_type.clone()]).build()?;
    broker.entities().merge(&building_x_id, &fragment).send().await?.done()?;
    let store = broker.with_accept(Accept::any()).entity_types().retrieve(&store_type).send().await?;
    expect_that!(store.entity_count, eq(1));
    expect_that!(store.attribute_details, len(eq(1)));
    expect_that!(attribute_names(&store), elements_are![eq(&height)]);
    expect_that!(families_of(&store, &height), some(elements_are![eq(&AttributeKind::Property)]));
    let information = broker.with_accept(Accept::any()).attributes().retrieve(&height).send().await?;
    expect_that!(information.attribute_count, some(eq(1)));
    expect_that!(information.attribute_types.as_deref(), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(
        information.type_names.as_deref(),
        some(unordered_elements_are![eq(&building_type), eq(&store_type)])
    );
    Ok(())
}
