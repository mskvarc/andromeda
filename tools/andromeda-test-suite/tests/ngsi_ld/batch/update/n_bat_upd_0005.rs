//! No overwrite appends a new attribute.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::Overwrite},
    model::{Entity, EntityFragment, NgsiLdMediaType},
};

/// Pins: a Batch Entity Update with `options=noOverwrite` whose fragment names only a new Attribute answers 204,
/// and the entity reads back with it
/// Clause: 5.6.9.4, 5.6.9.5, 5.6.3.4, 6.16.3.1
#[gtest]
#[tokio::test]
async fn n_bat_upd_0005_no_overwrite_appends_a_new_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let city_paris_id = namespace.urn("City", "Paris")?;
    let building = Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?;
    broker.entities().create(&building).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(building_id.clone(), building_type.clone())
        .relationship("locatedAt", city_paris_id.clone())
        .build()?];
    broker.entities().batch_update(&fragments).overwrite(Overwrite::Keep).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("locatedAt", any_attribute().object(city_paris_id.clone())));
    Ok(())
}
