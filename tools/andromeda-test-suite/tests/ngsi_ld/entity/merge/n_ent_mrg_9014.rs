//! A merge with the null scope deletes it.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, EntityFragment, NgsiLdMediaType, ScopeChange, Value},
    query::scope_query::ScopeQuery,
};

/// Pins: Merge Entity with scope "urn:ngsi-ld:null" onto scope `["/X/A","/Y/A"]` leaves the Entity without a scope,
/// and `scopeQ=/X/A` selects nothing
/// Clause: 4.18, 5.5.12, 5.6.17.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9014_a_merge_with_the_null_scope_deletes_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_null_id = namespace.urn("Building", "null")?;
    let building_type = namespace.entity_type("Building")?;
    let building_null = Entity::builder()
        .id(building_null_id.clone())
        .entity_type(building_type.clone())
        .scope(["/X/A", "/Y/A"])
        .property("temperature", Value::Number("21.5".parse()?))
        .build()?;
    broker.entities().create(&building_null).send().await?.done()?;
    let fragment = EntityFragment::builder().scope(ScopeChange::Delete).build()?;
    broker.entities().merge(&building_null_id, &fragment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_null_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.scope.as_ref(), none());
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_null_id.clone()])
        .scope_q("/X/A".parse::<ScopeQuery>()?)
        .count(ResultsCount::Include)
        .limit(0)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(0)));
    Ok(())
}
