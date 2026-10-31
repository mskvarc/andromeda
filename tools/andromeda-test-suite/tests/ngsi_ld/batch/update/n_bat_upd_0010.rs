//! A repeated id applies each instance in array order.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: a Batch Entity Update naming one entity twice, each instance updating a different Attribute, answers 204
/// and the entity keeps both updates, the later applied to the earlier one's result
/// Clause: 5.5.11.0, 5.5.11.3, 5.6.9.4, 6.16.3.1
#[gtest]
#[tokio::test]
async fn n_bat_upd_0010_a_repeated_id_applies_each_instance_in_array_order() -> Result<()> {
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
    let fragments = [
        EntityFragment::for_entity(building_id.clone(), building_type.clone())
            .property("name", "Tour Eiffel")
            .build()?,
        EntityFragment::for_entity(building_id.clone(), building_type.clone())
            .property("subCategory", "landmark")
            .build()?,
    ];
    broker.entities().batch_update(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Tour Eiffel")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("landmark")));
    Ok(())
}
