//! Deleting one of three instances keeps the other two and the attributes after them.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::InstanceSelection},
    model::{AttributeName, Entity, NgsiLdMediaType, Property},
};

/// Pins: Delete Attribute with a datasetId removes that instance of an Attribute holding a default and two dataset
/// instances; the default instance, the other dataset instance and the Attribute written after them stand as they
/// were
/// Clause: 5.6.5.4, 4.5.5, 6.7.3.2
#[gtest]
#[tokio::test]
async fn n_att_del_0014_deleting_one_of_three_instances_keeps_the_other_two_and_the_attributes_after_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let dataset_1 = namespace.dataset_id("Dataset", 1)?;
    let dataset_1_id = namespace.dataset_id("Dataset", 1)?;
    let dataset_2_id = namespace.dataset_id("Dataset", 2)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("temperature", 20)
        .property("temperature", Property::builder(21).dataset_id(namespace.dataset_id("Dataset", 1)?))
        .property("temperature", Property::builder(22).dataset_id(namespace.dataset_id("Dataset", 2)?))
        .property("humidity", 40)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    broker
        .entities()
        .delete_attribute(&building_id, &"temperature".parse::<AttributeName>()?)
        .instances(InstanceSelection::Dataset(&dataset_1))
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("temperature", 2));
    expect_that!(entity, not(has_attribute("temperature", any_attribute().dataset_id(dataset_1_id))));
    expect_that!(entity, has_attribute("temperature", any_attribute().dataset_id(dataset_2_id).value(22)));
    expect_that!(entity, has_instance("temperature", any_attribute().value(20).without(Member::DatasetId)));
    expect_that!(entity, has_attribute("humidity", any_attribute().value(40)));
    Ok(())
}
