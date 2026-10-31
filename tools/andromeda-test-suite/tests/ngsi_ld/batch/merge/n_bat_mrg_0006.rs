//! A repeated id merges the instances in array order.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: a Batch Entity Merge naming one entity twice, each instance merging a different Attribute, answers 204 and
/// the entity keeps both merges, the later composed onto the earlier one's result
/// Clause: 5.5.11.0, 5.5.11.5, 5.6.20.4
#[gtest]
#[tokio::test]
async fn n_bat_mrg_0006_a_repeated_id_merges_the_instances_in_array_order() -> Result<()> {
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
    broker.entities().batch_merge(&fragments).send().await?.done()?;
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
