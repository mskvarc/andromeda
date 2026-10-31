//! A null fragment selects the default or the named instance.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_attribute, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, AttributeName, Change, Entity, InstanceDeletion, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update with a null value and no `datasetId` deletes only the default instance, and with
/// a `datasetId` deletes only that instance, leaving the default one
/// Clause: 4.5.5, 5.5.8, 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9014_a_null_fragment_selects_the_default_or_the_named_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_delete_default_instance_id = namespace.urn("Building", "delete-default-instance")?;
    let building_type = namespace.entity_type("Building")?;
    let dataset_01 = namespace.dataset_id("Dataset", "01")?;
    let building_delete_named_instance_id = namespace.urn("Building", "delete-named-instance")?;
    let building_delete_default_instance = Entity::builder()
        .id(building_delete_default_instance_id.clone())
        .entity_type(building_type.clone())
        .property("street", 4)
        .property("street", Property::builder(4).dataset_id(namespace.dataset_id("Dataset", "01")?))
        .build()?;
    broker.entities().create(&building_delete_default_instance).send().await?.done()?;
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Property));
    broker
        .entities()
        .update_attribute(&building_delete_default_instance_id, &"street".parse::<AttributeName>()?, deletion)
        .send()
        .await?
        .done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_delete_default_instance_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("street", 1));
    expect_that!(entity, has_attribute("street", any_attribute().with_dataset_id(dataset_01.clone())));
    let building_delete_named_instance = Entity::builder()
        .id(building_delete_named_instance_id.clone())
        .entity_type(building_type.clone())
        .property("street", 4)
        .property("street", Property::builder(4).dataset_id(namespace.dataset_id("Dataset", "01")?))
        .build()?;
    broker.entities().create(&building_delete_named_instance).send().await?.done()?;
    let deletion_2 = Change::Delete(InstanceDeletion::new(AttributeKind::Property).with_dataset_id(dataset_01.clone()));
    broker
        .entities()
        .update_attribute(&building_delete_named_instance_id, &"street".parse::<AttributeName>()?, deletion_2)
        .send()
        .await?
        .done()?;
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_delete_named_instance_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_instances("street", 1));
    expect_that!(entity_2, has_attribute("street", any_attribute().value(4).without(Member::DatasetId)));
    Ok(())
}
