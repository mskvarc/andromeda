//! A merge with the null scope deletes a single scope.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, ScopeChange, Value},
};

/// Pins: Merge Entity with scope "urn:ngsi-ld:null" onto the scope /a/b/c answers 204 and leaves the Entity without
/// a scope
/// Clause: 4.18, 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9016_a_merge_with_the_null_scope_deletes_a_single_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_merge_null_id = namespace.urn("Building", "merge-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_merge_null = Entity::builder()
        .id(building_merge_null_id.clone())
        .entity_type(building_type)
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_merge_null).send().await?.done()?;
    let fragment = EntityFragment::builder().scope(ScopeChange::Delete).build()?;
    broker.entities().merge(&building_merge_null_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_merge_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), none());
    Ok(())
}
