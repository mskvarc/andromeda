//! One scope reads back as a string.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, spelling_matcher::alone};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Scope},
};

/// Pins: an entity created with one scope reads back with that scope as a single string
/// Clause: 5.6.1, 4.18, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ent_cre_0007_one_scope_reads_back_as_a_string() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.scope.as_ref(), some(alone(&"/Madrid/Gardens/ParqueNorte".parse::<Scope>()?)));
    Ok(())
}
