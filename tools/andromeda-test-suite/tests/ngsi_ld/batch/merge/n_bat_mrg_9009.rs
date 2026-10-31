//! A repeated id merges each instance onto the previous result.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: Batch Entity Merge naming an Entity with name "Eiffel Tower" twice, first merging name "Tour Eiffel" and
/// then `subCategory` "landmark", answers 204 and the Entity holds both merges
/// Clause: 5.5.11.0, 5.5.11.5, 5.6.20.4
#[gtest]
#[tokio::test]
async fn n_bat_mrg_9009_a_repeated_id_merges_each_instance_onto_the_previous_result() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merge_id = namespace.urn("Building", "merge")?;
    let building_type = namespace.entity_type("Building")?;
    let building_merge = Entity::builder()
        .id(building_merge_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?;
    broker.entities().create(&building_merge).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(building_merge_id.clone(), building_type.clone())
            .property("name", "Tour Eiffel")
            .build()?,
        EntityFragment::for_entity(building_merge_id.clone(), building_type.clone())
            .property("subCategory", "landmark")
            .build()?,
    ];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("name", any_attribute().value("Tour Eiffel")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("landmark")));
    Ok(())
}
