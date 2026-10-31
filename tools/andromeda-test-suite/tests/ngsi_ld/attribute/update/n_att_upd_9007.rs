//! An update with the null scope deletes it.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, NgsiLdMediaType, ScopeChange, Value},
};

/// Pins: Update Entity Attributes with scope "urn:ngsi-ld:null" answers 204 and leaves the Entity without a scope
/// Clause: 4.18, 5.5.8, 5.6.2.4
#[gtest]
#[tokio::test]
async fn n_att_upd_9007_an_update_with_the_null_scope_deletes_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_update_null_id = namespace.urn("Building", "update-null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_update_null = Entity::builder()
        .id(building_update_null_id.clone())
        .entity_type(building_type)
        .scope("/a/b/c")
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_update_null).send().await?.done()?;
    let fragment = EntityFragment::builder().scope(ScopeChange::Delete).build()?;
    broker.entities().update_attributes(&building_update_null_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_update_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), none());
    Ok(())
}
