//! Counts each instance once.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{
        AttributeKind,
        Entity,
        ErrorType,
        Property,
        geometry::{Position, Shape},
    },
};

/// Pins: `attributeCount` counts attribute instances: each `datasetId` instance apart and an entity carrying two
/// types once; deleting one `datasetId` instance, then every entity of one type, lowers the count and narrows the
/// families and type names to what remains
/// Clause: 5.7.10.4, 5.2.28, 4.5.5, 4.16, 5.6.5.4
#[gtest]
#[tokio::test]
async fn n_dsc_ati_0003_counts_each_instance_once() -> Result<()> {
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
    let dataset_x = namespace.dataset_id("Dataset", "x")?;
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
            .id(store_3_id.clone())
            .entity_type(store_type.clone())
            .relationship(namespace.attribute("marker")?, building_id.clone())
            .build()?,
        Entity::builder()
            .id(store_4_id.clone())
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
    let information = broker.with_accept(Accept::any()).attributes().retrieve(&marker).send().await?;
    expect_that!(information.attribute_count, some(eq(6)));
    expect_that!(
        information.attribute_types.as_deref(),
        some(unordered_elements_are![eq(&AttributeKind::Property), eq(&AttributeKind::Relationship)])
    );
    expect_that!(
        information.type_names.as_deref(),
        some(unordered_elements_are![eq(&building_type), eq(&store_type)])
    );
    broker
        .entities()
        .delete_attribute(&store_4_id, &marker)
        .instances(InstanceSelection::Dataset(&dataset_x))
        .send()
        .await?
        .done()?;
    let information_2 = broker.with_accept(Accept::any()).attributes().retrieve(&marker).send().await?;
    expect_that!(information_2.attribute_count, some(eq(5)));
    broker.entities().batch_delete(&[store_3_id, store_4_id]).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entity_types()
        .retrieve(&store_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let information_3 = broker.with_accept(Accept::any()).attributes().retrieve(&marker).send().await?;
    expect_that!(information_3.attribute_count, some(eq(3)));
    expect_that!(information_3.attribute_types.as_deref(), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(information_3.type_names.as_deref(), some(elements_are![eq(&building_type)]));
    Ok(())
}
