//! A null fragment naming an absent instance is not found.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, AttributeName, Change, Entity, ErrorType, InstanceDeletion, NgsiLdMediaType, Property},
};

/// Pins: Partial Attribute Update with a null value answers 404 `ResourceNotFound` for a `datasetId` naming no
/// instance and for an attribute the Entity does not carry, and changes nothing
/// Clause: 5.6.4.4
#[gtest]
#[tokio::test]
async fn n_att_pat_9016_a_null_fragment_naming_an_absent_instance_is_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_absent_instance_id = namespace.urn("Building", "absent-instance")?;
    let building_type = namespace.entity_type("Building")?;
    let dataset_02 = namespace.dataset_id("Dataset", "02")?;
    let dataset_01 = namespace.dataset_id("Dataset", "01")?;
    let building_absent_instance = Entity::builder()
        .id(building_absent_instance_id.clone())
        .entity_type(building_type)
        .property("street", Property::builder(4).dataset_id(namespace.dataset_id("Dataset", "01")?))
        .build()?;
    broker.entities().create(&building_absent_instance).send().await?.done()?;
    let deletion = Change::Delete(InstanceDeletion::new(AttributeKind::Property).with_dataset_id(dataset_02));
    let answer = broker
        .with_responses()
        .entities()
        .update_attribute(&building_absent_instance_id, &"street".parse::<AttributeName>()?, deletion)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let deletion_2 = Change::Delete(InstanceDeletion::new(AttributeKind::Property));
    let answer_2 = broker
        .with_responses()
        .entities()
        .update_attribute(&building_absent_instance_id, &"colour".parse::<AttributeName>()?, deletion_2)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_absent_instance_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("street", any_attribute().with_dataset_id(dataset_01).value(4)));
    Ok(())
}
