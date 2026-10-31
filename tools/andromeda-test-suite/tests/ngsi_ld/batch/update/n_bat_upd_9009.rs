//! A repeated id updates each instance onto the previous result.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Batch Entity Update naming an Entity with name and `subCategory` twice, first updating name to "Tour
/// Eiffel" and then `subCategory` to "landmark", answers 204 and the Entity holds both updates
/// Clause: 5.5.11.0, 5.5.11.3, 5.6.9.4
#[gtest]
#[tokio::test]
async fn n_bat_upd_9009_a_repeated_id_updates_each_instance_onto_the_previous_result() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_update_id = namespace.urn("Building", "update")?;
    let building_type = namespace.entity_type("Building")?;
    let building_update = Entity::builder()
        .id(building_update_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .build()?;
    broker.entities().create(&building_update).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_update_id.clone(), building_type.clone())
            .property("name", "Tour Eiffel")
            .build()?,
        EntityFragment::for_entity(building_update_id.clone(), building_type.clone())
            .property("subCategory", "landmark")
            .build()?,
    ];
    broker.entities().batch_update(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_update_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Tour Eiffel")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("landmark")));
    Ok(())
}
