//! A batch of three hundred and its deletion are counted exactly.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, Entity, Input, Property},
};

/// Pins: a batch create of three hundred entities in one request is counted exactly, by type, by attribute
/// instance, family and type name, and so is a batch delete of half of them
/// Clause: 5.7.7.4, 5.7.10.4, 5.2.28, 5.6.7.4, 5.6.10.4, 4.16
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0008_a_batch_of_three_hundred_and_its_deletion_are_counted_exactly() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let store_type = namespace.entity_type("Store")?;
    let building_0_id = namespace.urn("Building", 0)?;
    let dataset_b = namespace.dataset_id("Dataset", "b")?;
    let marker = namespace.attribute("marker")?;
    let level = namespace.attribute("level")?;
    let entities = (0..300)
        .map(|index| -> Result<Entity<Input>> {
            let id = namespace.urn("Building", index)?;
            if index % 3 == 0 {
                Ok(Entity::builder()
                    .id(id)
                    .entity_type(vec![building_type.clone(), store_type.clone()])
                    .property(namespace.attribute("marker")?, 1)
                    .property(namespace.attribute("level")?, 2)
                    .property(namespace.attribute("level")?, Property::builder(3).dataset_id(dataset_b.clone()))
                    .build()?)
            } else {
                Ok(Entity::builder()
                    .id(id)
                    .entity_type(building_type.clone())
                    .relationship(namespace.attribute("marker")?, building_0_id.clone())
                    .build()?)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let created = broker.entities().batch_create(&entities).send().await?.done()?;
    expect_that!(created, len(eq(300)));
    let building = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(building.entity_count, eq(300));
    let store = broker.with_accept(Accept::any()).entity_types().retrieve(&store_type).send().await?;
    expect_that!(store.entity_count, eq(100));
    let level_information = broker.with_accept(Accept::any()).attributes().retrieve(&level).send().await?;
    expect_that!(level_information.attribute_count, some(eq(200)));
    expect_that!(level_information.attribute_types.as_deref(), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(
        level_information.type_names.as_deref(),
        some(unordered_elements_are![eq(&building_type), eq(&store_type)])
    );
    let marker_information = broker.with_accept(Accept::any()).attributes().retrieve(&marker).send().await?;
    expect_that!(marker_information.attribute_count, some(eq(300)));
    expect_that!(
        marker_information.attribute_types.as_deref(),
        some(unordered_elements_are![eq(&AttributeKind::Property), eq(&AttributeKind::Relationship)])
    );
    expect_that!(
        marker_information.type_names.as_deref(),
        some(unordered_elements_are![eq(&building_type), eq(&store_type)])
    );
    let half = (0..150).map(|index| Ok(namespace.urn("Building", index)?)).collect::<Result<Vec<_>>>()?;
    broker.entities().batch_delete(&half).send().await?.done()?;
    let building_2 = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(building_2.entity_count, eq(150));
    let store_2 = broker.with_accept(Accept::any()).entity_types().retrieve(&store_type).send().await?;
    expect_that!(store_2.entity_count, eq(50));
    Ok(())
}
