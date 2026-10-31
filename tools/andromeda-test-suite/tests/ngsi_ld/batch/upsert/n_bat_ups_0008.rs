//! Replacing drops a scope the item omits.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
};

/// Pins: a Batch Entity Upsert without options of an entity holding a scope, with an item carrying only id and
/// type, answers 204 and the entity no longer has a scope
/// Clause: 5.6.8.4, 5.6.8.5, 4.18, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0008_replacing_drops_a_scope_the_item_omits() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let entities = [Entity::builder().id(building_id.clone()).entity_type(building_type.clone()).build()?];
    broker.entities().batch_upsert(&entities).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.scope.as_ref(), none());
    Ok(())
}
