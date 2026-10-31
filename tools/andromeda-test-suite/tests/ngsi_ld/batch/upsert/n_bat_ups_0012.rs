//! Update mode keeps the scope.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    spelling_matcher::alone,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::UpsertMode},
    model::{Entity, NgsiLdMediaType, Scope},
};

/// Pins: a Batch Entity Upsert with options=update of an entity holding a scope, with an item carrying Properties
/// and no scope, answers 204 and the entity keeps its scope beside the new Properties
/// Clause: 5.6.8.4, 5.6.8.5, 5.6.2.4, 4.18, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0012_update_mode_keeps_the_scope() -> Result<()> {
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
    let entities = [Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .build()?];
    broker.entities().batch_upsert(&entities).mode(UpsertMode::Update).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), some(alone(&"/Madrid/Gardens/ParqueNorte".parse::<Scope>()?)));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    Ok(())
}
