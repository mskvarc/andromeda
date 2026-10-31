//! Dataset id none is the default instance.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    attribute_member::Member,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DatasetId, Entity, NgsiLdMediaType, Property},
};

/// Pins: Batch Entity Upsert accepts "`datasetId`": "@none" on a Property of a new entity and of an existing
/// entity, replacing the default instance of the existing one
/// Clause: 4.5.5, 5.6.8
#[gtest]
#[tokio::test]
async fn n_bat_ups_2301_dataset_id_none_is_the_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_old_id = namespace.urn("Vehicle", "old")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_new_id = namespace.urn("Vehicle", "new")?;
    let vehicle_old = Entity::builder()
        .id(vehicle_old_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle_old).send().await?.done()?;
    let entities = [
        Entity::builder()
            .id(vehicle_old_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(9).dataset_id("@none".parse::<DatasetId>()?))
            .build()?,
        Entity::builder()
            .id(vehicle_new_id.clone())
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(5).dataset_id("@none".parse::<DatasetId>()?))
            .build()?,
    ];
    let answer = broker.with_responses().entities().batch_upsert(&entities).send().await;
    expect_that!(answer.answered_status().map(|status| status.as_u16()), some(all!(ge(200), lt(300))));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_old_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(9).without(Member::DatasetId)));
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_new_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2, has_attribute("speed", any_attribute().value(5).without(Member::DatasetId)));
    Ok(())
}
