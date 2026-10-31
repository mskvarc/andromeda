//! A no overwrite instance of a new dataset id is appended to a held attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::{has_instance, has_instances},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Update with `options=noOverwrite` of a held Attribute written under a `datasetId` the entity
/// does not hold answers 204 and appends that instance beside the held default one
/// Clause: 5.6.3.4, 5.6.9.4, 5.6.9.5
#[gtest]
#[tokio::test]
async fn n_bat_upd_9014_a_no_overwrite_instance_of_a_new_dataset_id_is_appended_to_a_held_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let french = namespace.dataset_id("Dataset", "french")?;
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_id.clone(), building_type.clone())
        .property("name", Property::builder("Tour Eiffel").dataset_id(french.clone()))
        .build()?];
    broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_instances("name", 2));
    expect_that!(entity, has_instance("name", any_attribute().with_dataset_id(french).value("Tour Eiffel")));
    expect_that!(entity, has_instance("name", any_attribute().without(Member::DatasetId).value("Eiffel Tower")));
    Ok(())
}
