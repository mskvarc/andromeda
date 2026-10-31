//! An entity without attributes answers 204 and changes nothing.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Batch Entity Merge whose only entity holds just its id and type, so no Attribute Fragment, answers 204 and
/// the stored values stay as created
/// Clause: 5.4, 5.6.20
/// Recorded: an entity of a batch merge is an IETF RFC 7396 JSON Merge Patch (clause 5.4); with no Attribute member
/// there is nothing to change and clause 5.6.20 lists no error for it
#[gtest]
#[tokio::test]
async fn n_bat_mrg_2701_an_entity_without_attributes_answers_204_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone()).build()?];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(55)));
    Ok(())
}
