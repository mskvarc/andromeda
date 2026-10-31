//! No overwrite of only present attributes answers 207 and keeps them.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    partial_outcome::PartialSuccess,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: a Batch Entity Update with `options=noOverwrite` whose fragment names only Attributes the entity already
/// holds appends nothing, answers 207 and leaves every value as it was
/// Clause: 5.6.9.4, 5.6.9.5, 5.6.3.4, 6.16.3.1
/// Recorded: clause 5.6.9.4 adds an entity to S when it "was successfully updated (Attributes appended)" and does
/// not say where an entity with nothing appended goes; Andromeda answers 207 for it
#[gtest]
#[tokio::test]
async fn o_bat_upd_0004_no_overwrite_of_only_present_attributes_answers_207_and_keeps_them() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_id.clone(), building_type.clone())
        .property("name", "Tour Eiffel")
        .property("subCategory", "landmark")
        .build()?];
    broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.partial()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    Ok(())
}
